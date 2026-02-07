#![allow(unused)]
//! Unix Domain Socket implementation for IPC communication
//! Provides efficient local communication on Unix-like systems

use super::*;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use tokio::time::Duration;

/// Unix Domain Socket protocol implementation
#[derive(Debug)]
pub struct UnixSocketProtocol;

#[async_trait::async_trait]
impl CommunicationProtocol for UnixSocketProtocol {
    fn protocol_type(&self) -> ProtocolType {
        ProtocolType::UnixSocket
    }

    async fn create_server(
        &self,
        config: &CommunicationConfig,
    ) -> Result<Box<dyn CommunicationServer>, CommunicationError> {
        #[cfg(unix)]
        {
            Ok(Box::new(UnixSocketServer::new(config)?))
        }
        #[cfg(not(unix))]
        {
            Err(CommunicationError::ConnectionFailed(
                "Unix socket protocol is not supported on Windows".to_string(),
            ))
        }
    }

    async fn create_client(
        &self,
        config: &CommunicationConfig,
    ) -> Result<Box<dyn CommunicationClient>, CommunicationError> {
        #[cfg(unix)]
        {
            Ok(Box::new(UnixSocketClient::new(config)?))
        }
        #[cfg(not(unix))]
        {
            Err(CommunicationError::ConnectionFailed(
                "Unix socket protocol is not supported on Windows".to_string(),
            ))
        }
    }
}

/// Unix Domain Socket server implementation
#[cfg(unix)]
pub struct UnixSocketServer {
    config: CommunicationConfig,
    listener: Arc<Mutex<Option<tokio::net::UnixListener>>>,
    is_running: Arc<Mutex<bool>>,
    broadcast_tx: tokio::sync::broadcast::Sender<CommunicationMessage>,
    message_handler: SharedMessageHandler,
}

#[cfg(unix)]
impl std::fmt::Debug for UnixSocketServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UnixSocketServer")
            .field("config", &self.config)
            .finish()
    }
}

#[cfg(unix)]
impl Default for UnixSocketServer {
    fn default() -> Self {
        let (broadcast_tx, _) = tokio::sync::broadcast::channel(100);
        Self {
            config: CommunicationConfig::default(),
            listener: Arc::new(Mutex::new(None)),
            is_running: Arc::new(Mutex::new(false)),
            broadcast_tx,
            message_handler: Arc::new(Mutex::new(None)),
        }
    }
}

#[cfg(unix)]
impl UnixSocketServer {
    pub fn new(config: &CommunicationConfig) -> Result<Self, CommunicationError> {
        let (broadcast_tx, _) = tokio::sync::broadcast::channel(100);
        Ok(Self {
            config: config.clone(),
            listener: Arc::new(Mutex::new(None)),
            is_running: Arc::new(Mutex::new(false)),
            broadcast_tx,
            message_handler: Arc::new(Mutex::new(None)),
        })
    }

    fn get_socket_path(identifier: &str) -> String {
        format!("/tmp/{}.sock", identifier)
    }

    async fn handle_client(
        &self,
        stream: tokio::net::UnixStream,
        mut broadcast_rx: tokio::sync::broadcast::Receiver<CommunicationMessage>,
    ) -> Result<(), CommunicationError> {
        use tokio::io::AsyncBufReadExt;
        let (reader, mut writer) = stream.into_split();
        let mut reader = tokio::io::BufReader::new(reader);

        loop {
            tokio::select! {
                // Read from client
                read_result = async {
                    match self.config.serialization_format {
                        SerializationFormat::Json => {
                            let mut line = String::new();
                            match reader.read_line(&mut line).await {
                                Ok(0) => Ok(None),
                                Ok(_) => Ok(Some(serde_json::from_str::<CommunicationMessage>(&line)
                                    .map_err(|e| CommunicationError::DeserializationFailed(e.to_string()))?)),
                                Err(e) => Err(CommunicationError::IoError(e.to_string())),
                            }
                        }
                        SerializationFormat::MsgPack => {
                            let mut len_bytes = [0u8; 4];
                            match reader.read_exact(&mut len_bytes).await {
                                Ok(_) => {
                                    let len = u32::from_be_bytes(len_bytes) as usize;
                                    let mut buf = vec![0u8; len];
                                    match reader.read_exact(&mut buf).await {
                                        Ok(_) => {
                                            Ok(Some(rmp_serde::from_slice::<CommunicationMessage>(&buf)
                                                .map_err(|e| CommunicationError::DeserializationFailed(e.to_string()))?))
                                        }
                                        Err(e) => Err(CommunicationError::IoError(e.to_string())),
                                    }
                                }
                                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(None),
                                Err(e) => Err(CommunicationError::IoError(e.to_string())),
                            }
                        }
                    }
                } => {
                    match read_result {
                        Ok(Some(message)) => {
                            crate::ipc_log!("Received message type: {}", message.message_type);

                            // Call message handler if set and get optional response
                            let response = if let Some(ref handler) = *self.message_handler.lock().await {
                                handler(message.clone())
                            } else {
                                None
                            };

                            // Default response if none provided by handler
                            let response = response.unwrap_or_else(|| {
                                message.create_reply(serde_json::json!("Received"))
                            });

                            // Broadcast to other clients (original message)
                            let _ = self.broadcast_tx.send(message);

                            // Send response back to client
                            match self.config.serialization_format {
                                SerializationFormat::Json => {
                                    let mut resp_json = serde_json::to_string(&response)?;
                                    resp_json.push('\n');
                                    let _ = writer.write_all(resp_json.as_bytes()).await;
                                }
                                SerializationFormat::MsgPack => {
                                    let resp_bytes = rmp_serde::to_vec(&response)
                                        .map_err(|e| CommunicationError::SerializationFailed(e.to_string()))?;
                                    let len = resp_bytes.len() as u32;
                                    let _ = writer.write_all(&len.to_be_bytes()).await;
                                    let _ = writer.write_all(&resp_bytes).await;
                                }
                            }
                        }
                        Ok(None) => break, // Connection closed
                        Err(_) => break, // Error or closed
                    }
                }
                // Receive broadcast and send to client
                broadcast_msg = broadcast_rx.recv() => {
                    match broadcast_msg {
                        Ok(msg) => {
                            match self.config.serialization_format {
                                SerializationFormat::Json => {
                                    let mut msg_json = serde_json::to_string(&msg)?;
                                    msg_json.push('\n');
                                    if writer.write_all(msg_json.as_bytes()).await.is_err() {
                                        break;
                                    }
                                }
                                SerializationFormat::MsgPack => {
                                    if let Ok(msg_bytes) = rmp_serde::to_vec(&msg) {
                                        let len = msg_bytes.len() as u32;
                                        if writer.write_all(&len.to_be_bytes()).await.is_err() ||
                                           writer.write_all(&msg_bytes).await.is_err() {
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(_) => break,
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(unix)]
#[async_trait::async_trait]
impl CommunicationServer for UnixSocketServer {
    async fn start(&mut self) -> Result<(), CommunicationError> {
        let socket_path = Self::get_socket_path(&self.config.identifier);

        crate::ipc_log!("UnixSocketServer starting on {}", socket_path);
        let listener_bound = tokio::net::UnixListener::bind(&socket_path)?;
        *self.listener.lock().await = Some(listener_bound);
        *self.is_running.lock().await = true;

        let listener = match self.listener.lock().await.take() {
            Some(l) => l,
            None => {
                return Err(CommunicationError::ConnectionFailed(
                    "Listener already taken or not initialized".to_string(),
                ));
            }
        };

        let is_running = self.is_running.clone();
        let socket_path_clone = socket_path.clone();
        let broadcast_tx = self.broadcast_tx.clone();
        let message_handler = self.message_handler.clone();
        let config = self.config.clone();

        // Spawn a task to handle incoming connections
        tokio::spawn(async move {
            while *is_running.lock().await {
                match listener.accept().await {
                    Ok((stream, _addr)) => {
                        let broadcast_rx = broadcast_tx.subscribe();
                        let broadcast_tx_inner = broadcast_tx.clone();
                        let message_handler_inner = message_handler.clone();
                        let config_inner = config.clone();
                        tokio::spawn(async move {
                            let server = UnixSocketServer {
                                config: config_inner,
                                listener: Arc::new(Mutex::new(None)),
                                is_running: Arc::new(Mutex::new(true)),
                                broadcast_tx: broadcast_tx_inner,
                                message_handler: message_handler_inner,
                            };
                            let _ = server.handle_client(stream, broadcast_rx).await;
                        });
                    }
                    Err(e) => {
                        if *is_running.lock().await {
                            eprintln!("Error accepting connection: {}", e);
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        } else {
                            break;
                        }
                    }
                }
            }
            let _ = std::fs::remove_file(&socket_path_clone);
        });

        Ok(())
    }

    async fn stop(&mut self) -> Result<(), CommunicationError> {
        *self.is_running.lock().await = false;
        *self.listener.lock().await = None;
        Ok(())
    }

    fn is_running(&self) -> bool {
        match self.is_running.try_lock() {
            Ok(running) => *running,
            Err(_) => true, // If locked, it's likely running
        }
    }

    fn endpoint(&self) -> String {
        Self::get_socket_path(&self.config.identifier)
    }

    async fn broadcast(&self, message: CommunicationMessage) -> Result<(), CommunicationError> {
        let _ = self.broadcast_tx.send(message);
        Ok(())
    }

    fn set_message_handler(&self, handler: MessageHandler) -> Result<(), CommunicationError> {
        let message_handler = self.message_handler.clone();
        tokio::spawn(async move {
            *message_handler.lock().await = Some(handler);
        });
        Ok(())
    }
}

/// Unix Domain Socket client implementation
#[cfg(unix)]
#[derive(Debug)]
pub struct UnixSocketClient {
    config: CommunicationConfig,
    reader: tokio::sync::Mutex<Option<tokio::io::BufReader<tokio::net::unix::OwnedReadHalf>>>,
    writer: tokio::sync::Mutex<Option<tokio::net::unix::OwnedWriteHalf>>,
    connected: std::sync::atomic::AtomicBool,
}

#[cfg(unix)]
impl UnixSocketClient {
    pub fn new(config: &CommunicationConfig) -> Result<Self, CommunicationError> {
        Ok(Self {
            config: config.clone(),
            reader: tokio::sync::Mutex::new(None),
            writer: tokio::sync::Mutex::new(None),
            connected: std::sync::atomic::AtomicBool::new(false),
        })
    }
}

#[cfg(unix)]
#[async_trait::async_trait]
impl CommunicationClient for UnixSocketClient {
    async fn connect(&mut self) -> Result<(), CommunicationError> {
        let socket_path = format!("/tmp/{}.sock", self.config.identifier);
        crate::ipc_log!("UnixSocketClient connecting to {}", socket_path);

        let stream = tokio::net::UnixStream::connect(&socket_path).await?;
        let (reader_half, writer_half) = stream.into_split();

        *self.reader.get_mut() = Some(tokio::io::BufReader::new(reader_half));
        *self.writer.get_mut() = Some(writer_half);
        self.connected
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    async fn send_message(&self, message: &CommunicationMessage) -> Result<(), CommunicationError> {
        if !self.connected.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(CommunicationError::ConnectionFailed(
                "Not connected".to_string(),
            ));
        }

        let mut writer_guard = self.writer.lock().await;
        if let Some(writer) = writer_guard.as_mut() {
            match self.config.serialization_format {
                SerializationFormat::Json => {
                    let mut message_json = serde_json::to_string(message)?;
                    message_json.push('\n');
                    writer.write_all(message_json.as_bytes()).await?;
                    Ok(())
                }
                SerializationFormat::MsgPack => {
                    let msg_bytes = rmp_serde::to_vec(message)
                        .map_err(|e| CommunicationError::SerializationFailed(e.to_string()))?;
                    let len = msg_bytes.len() as u32;
                    writer.write_all(&len.to_be_bytes()).await?;
                    writer.write_all(&msg_bytes).await?;
                    Ok(())
                }
            }
        } else {
            Err(CommunicationError::ConnectionFailed(
                "Writer not initialized".to_string(),
            ))
        }
    }

    async fn receive_message(&self) -> Result<CommunicationMessage, CommunicationError> {
        if !self.connected.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(CommunicationError::ConnectionFailed(
                "Not connected".to_string(),
            ));
        }

        use tokio::io::AsyncBufReadExt;
        use tokio::io::AsyncReadExt;
        let mut reader_guard = self.reader.lock().await;

        if let Some(reader) = reader_guard.as_mut() {
            match self.config.serialization_format {
                SerializationFormat::Json => {
                    let mut line = String::new();
                    let bytes_read = reader.read_line(&mut line).await?;
                    if bytes_read == 0 {
                        return Err(CommunicationError::ConnectionFailed(
                            "Connection closed".to_string(),
                        ));
                    }
                    let message: CommunicationMessage =
                        serde_json::from_str(&line).map_err(|e| {
                            CommunicationError::DeserializationFailed(format!("{}: {}", e, line))
                        })?;
                    Ok(message)
                }
                SerializationFormat::MsgPack => {
                    let mut len_bytes = [0u8; 4];
                    match reader.read_exact(&mut len_bytes).await {
                        Ok(_) => {
                            let len = u32::from_be_bytes(len_bytes) as usize;
                            let mut buf = vec![0u8; len];
                            match reader.read_exact(&mut buf).await {
                                Ok(_) => {
                                    let message: CommunicationMessage = rmp_serde::from_slice(&buf)
                                        .map_err(|e| {
                                            CommunicationError::DeserializationFailed(e.to_string())
                                        })?;
                                    Ok(message)
                                }
                                Err(e) => Err(CommunicationError::IoError(e.to_string())),
                            }
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Err(
                            CommunicationError::ConnectionFailed("Connection closed".to_string()),
                        ),
                        Err(e) => Err(CommunicationError::IoError(e.to_string())),
                    }
                }
            }
        } else {
            Err(CommunicationError::ConnectionFailed(
                "Reader not initialized".to_string(),
            ))
        }
    }

    async fn disconnect(&mut self) -> Result<(), CommunicationError> {
        *self.reader.get_mut() = None;
        *self.writer.get_mut() = None;
        self.connected
            .store(false, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected.load(std::sync::atomic::Ordering::SeqCst)
    }
}
