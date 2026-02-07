//! Unix Domain Socket implementation for IPC communication
//! Provides efficient local communication on Unix-like systems

use super::*;
use crate::ipc_log;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::{UnixListener, UnixStream};
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
        Ok(Box::new(UnixSocketServer::new(config)?))
    }

    async fn create_client(
        &self,
        config: &CommunicationConfig,
    ) -> Result<Box<dyn CommunicationClient>, CommunicationError> {
        Ok(Box::new(UnixSocketClient::new(config)?))
    }
}

/// Unix Domain Socket server implementation
pub struct UnixSocketServer {
    config: CommunicationConfig,
    listener: Arc<Mutex<Option<UnixListener>>>,
    is_running: Arc<Mutex<bool>>,
    broadcast_tx: tokio::sync::broadcast::Sender<CommunicationMessage>,
    message_handler: SharedMessageHandler,
}

impl std::fmt::Debug for UnixSocketServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UnixSocketServer")
            .field("config", &self.config)
            .finish()
    }
}

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
        stream: UnixStream,
        mut broadcast_rx: tokio::sync::broadcast::Receiver<CommunicationMessage>,
    ) -> Result<(), CommunicationError> {
        use tokio::io::AsyncBufReadExt;
        let (reader, mut writer) = stream.into_split();
        let mut reader = tokio::io::BufReader::new(reader);
        let mut line = String::new();

        loop {
            tokio::select! {
                // Read from client
                read_result = reader.read_line(&mut line) => {
                    match read_result {
                        Ok(bytes_read) => {
                            if bytes_read == 0 { break; }

                            let message_res = serde_json::from_str::<CommunicationMessage>(&line);
                            line.clear();

                            if let Ok(message) = message_res {
                                ipc_log!("Received message type: {}", message.message_type);

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
                                let mut resp_json = serde_json::to_string(&response)?;
                                resp_json.push('\n');
                                let _ = writer.write_all(resp_json.as_bytes()).await;
                            }
                        }
                        Err(_) => break,
                    }
                }
                // Receive broadcast and send to client
                broadcast_msg = broadcast_rx.recv() => {
                    match broadcast_msg {
                        Ok(msg) => {
                            let mut msg_json = serde_json::to_string(&msg)?;
                            msg_json.push('\n');
                            if writer.write_all(msg_json.as_bytes()).await.is_err() {
                                break;
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

#[async_trait::async_trait]
impl CommunicationServer for UnixSocketServer {
    async fn start(&mut self) -> Result<(), CommunicationError> {
        let socket_path = Self::get_socket_path(&self.config.identifier);

        ipc_log!("UnixSocketServer starting on {}", socket_path);
        let listener_bound = UnixListener::bind(&socket_path)?;
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
#[derive(Debug)]
pub struct UnixSocketClient {
    config: CommunicationConfig,
    reader: tokio::sync::Mutex<Option<tokio::io::BufReader<tokio::net::unix::OwnedReadHalf>>>,
    writer: tokio::sync::Mutex<Option<tokio::net::unix::OwnedWriteHalf>>,
    connected: std::sync::atomic::AtomicBool,
}

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

#[async_trait::async_trait]
impl CommunicationClient for UnixSocketClient {
    async fn connect(&mut self) -> Result<(), CommunicationError> {
        let socket_path = format!("/tmp/{}.sock", self.config.identifier);
        ipc_log!("UnixSocketClient connecting to {}", socket_path);

        let stream = UnixStream::connect(&socket_path).await?;
        let (reader_half, writer_half) = stream.into_split();

        *self.reader.get_mut() = Some(tokio::io::BufReader::new(reader_half));
        *self.writer.get_mut() = Some(writer_half);
        self.connected.store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    async fn send_message(
        &self,
        message: &CommunicationMessage,
    ) -> Result<(), CommunicationError> {
        if !self.connected.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(CommunicationError::ConnectionFailed(
                "Not connected".to_string(),
            ));
        }

        let mut writer_guard = self.writer.lock().await;
        if let Some(writer) = writer_guard.as_mut() {
            let mut message_json = serde_json::to_string(message)?;
            message_json.push('\n');
            writer.write_all(message_json.as_bytes()).await?;
            Ok(())
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
        let mut reader_guard = self.reader.lock().await; 
        
        if let Some(reader) = reader_guard.as_mut() {
            let mut line = String::new();
            let bytes_read = reader.read_line(&mut line).await?;
            if bytes_read == 0 {
                return Err(CommunicationError::ConnectionFailed(
                    "Connection closed".to_string(),
                ));
            }

            let message: CommunicationMessage = serde_json::from_str(&line)
                .map_err(|e| CommunicationError::DeserializationFailed(format!("{}: {}", e, line)))?;

            Ok(message)
        } else {
             Err(CommunicationError::ConnectionFailed(
                "Reader not initialized".to_string(),
            ))
        }
    }

    async fn disconnect(&mut self) -> Result<(), CommunicationError> {
        *self.reader.get_mut() = None;
        *self.writer.get_mut() = None;
        self.connected.store(false, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected.load(std::sync::atomic::Ordering::SeqCst)
    }
}
