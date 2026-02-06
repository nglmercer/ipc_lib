//! Unix Domain Socket implementation for IPC communication
//! Provides efficient local communication on Unix-like systems

use super::*;
use crate::ipc_log;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};

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
#[derive(Debug)]
pub struct UnixSocketServer {
    config: CommunicationConfig,
    listener: Arc<Mutex<Option<UnixListener>>>,
    is_running: Arc<Mutex<bool>>,
    broadcast_tx: tokio::sync::broadcast::Sender<CommunicationMessage>,
}

impl Default for UnixSocketServer {
    fn default() -> Self {
        let (broadcast_tx, _) = tokio::sync::broadcast::channel(100);
        Self {
            config: CommunicationConfig::default(),
            listener: Arc::new(Mutex::new(None)),
            is_running: Arc::new(Mutex::new(false)),
            broadcast_tx,
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
                read_result = timeout(Duration::from_millis(self.config.timeout_ms), reader.read_line(&mut line)) => {
                    match read_result {
                        Ok(Ok(bytes_read)) => {
                            if bytes_read == 0 { break; }

                            let message_res = serde_json::from_str::<CommunicationMessage>(&line);
                            line.clear();

                            if let Ok(message) = message_res {
                                match message.message_type.as_str() {
                                    "chat" => {
                                        if let Some(content) = message.payload.as_str() {
                                            // Printing here shows messages on the host's terminal
                                            println!("\r[CHAT] {}: {}", message.source_id, content);
                                            print!("> ");
                                            let _ = std::io::Write::flush(&mut std::io::stdout());

                                            // Broadcast to all other clients
                                            let _ = self.broadcast_tx.send(message);
                                        }
                                    }
                                    _ => {
                                        let response = CommunicationMessage::response("Received".to_string());
                                        let mut resp_json = serde_json::to_string(&response)?;
                                        resp_json.push('\n');
                                        let _ = writer.write_all(resp_json.as_bytes()).await;
                                    }
                                }
                            }
                        }
                        _ => break,
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
        let listener = UnixListener::bind(&socket_path)?;
        *self.listener.lock().await = Some(listener);
        *self.is_running.lock().await = true;

        let is_running = self.is_running.clone();
        let listener = self.listener.clone();
        let socket_path_clone = socket_path.clone();
        let broadcast_tx = self.broadcast_tx.clone();
        let config = self.config.clone();

        // Spawn a task to handle incoming connections
        tokio::spawn(async move {
            while *is_running.lock().await {
                if let Some(l) = listener.lock().await.as_ref() {
                    match l.accept().await {
                        Ok((stream, _addr)) => {
                            let broadcast_rx = broadcast_tx.subscribe();
                            let broadcast_tx_inner = broadcast_tx.clone();
                            let config_inner = config.clone();
                            tokio::spawn(async move {
                                let server = UnixSocketServer {
                                    config: config_inner,
                                    listener: Arc::new(Mutex::new(None)),
                                    is_running: Arc::new(Mutex::new(true)),
                                    broadcast_tx: broadcast_tx_inner,
                                };
                                let _ = server.handle_client(stream, broadcast_rx).await;
                            });
                        }
                        Err(_) => break,
                    }
                } else {
                    break;
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
        *self.is_running.blocking_lock()
    }

    fn endpoint(&self) -> String {
        Self::get_socket_path(&self.config.identifier)
    }

    async fn broadcast(&self, message: CommunicationMessage) -> Result<(), CommunicationError> {
        let _ = self.broadcast_tx.send(message);
        Ok(())
    }
}

/// Unix Domain Socket client implementation
#[derive(Debug)]
pub struct UnixSocketClient {
    config: CommunicationConfig,
    reader: Option<tokio::io::BufReader<tokio::net::unix::OwnedReadHalf>>,
    writer: Option<tokio::net::unix::OwnedWriteHalf>,
    connected: bool,
}

impl UnixSocketClient {
    pub fn new(config: &CommunicationConfig) -> Result<Self, CommunicationError> {
        Ok(Self {
            config: config.clone(),
            reader: None,
            writer: None,
            connected: false,
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

        self.reader = Some(tokio::io::BufReader::new(reader_half));
        self.writer = Some(writer_half);
        self.connected = true;
        Ok(())
    }

    async fn send_message(
        &mut self,
        message: &CommunicationMessage,
    ) -> Result<(), CommunicationError> {
        if !self.connected {
            return Err(CommunicationError::ConnectionFailed(
                "Not connected".to_string(),
            ));
        }

        let writer = self.writer.as_mut().unwrap();
        let mut message_json = serde_json::to_string(message)?;
        message_json.push('\n');
        writer.write_all(message_json.as_bytes()).await?;
        Ok(())
    }

    async fn receive_message(&mut self) -> Result<CommunicationMessage, CommunicationError> {
        if !self.connected {
            return Err(CommunicationError::ConnectionFailed(
                "Not connected".to_string(),
            ));
        }

        use tokio::io::AsyncBufReadExt;
        let reader = self.reader.as_mut().unwrap();
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
    }

    async fn disconnect(&mut self) -> Result<(), CommunicationError> {
        self.reader = None;
        self.writer = None;
        self.connected = false;
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected
    }
}
