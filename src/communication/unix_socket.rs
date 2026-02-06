//! Unix Domain Socket implementation for IPC communication
//! Provides efficient local communication on Unix-like systems

use super::*;
use crate::ipc_log;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
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
}

impl Default for UnixSocketServer {
    fn default() -> Self {
        Self {
            config: CommunicationConfig::default(),
            listener: Arc::new(Mutex::new(None)),
            is_running: Arc::new(Mutex::new(false)),
        }
    }
}

impl UnixSocketServer {
    pub fn new(config: &CommunicationConfig) -> Result<Self, CommunicationError> {
        Ok(Self {
            config: config.clone(),
            listener: Arc::new(Mutex::new(None)),
            is_running: Arc::new(Mutex::new(false)),
        })
    }

    fn get_socket_path(identifier: &str) -> String {
        format!("/tmp/{}.sock", identifier)
    }

    async fn handle_client(&self, mut stream: UnixStream) -> Result<(), CommunicationError> {
        let mut buffer = vec![0u8; 4096];

        loop {
            match timeout(
                Duration::from_millis(self.config.timeout_ms),
                stream.read(&mut buffer),
            )
            .await
            {
                Ok(Ok(bytes_read)) => {
                    if bytes_read == 0 {
                        // Client disconnected
                        break;
                    }

                    let message_str = String::from_utf8_lossy(&buffer[..bytes_read]);
                    match serde_json::from_str::<CommunicationMessage>(&message_str) {
                        Ok(message) => match message.message_type.as_str() {
                            "command_line_args" => {
                                let response = CommunicationMessage::response(
                                    "Command line arguments received successfully".to_string(),
                                );
                                let response_json = serde_json::to_string(&response)?;
                                stream.write_all(response_json.as_bytes()).await?;
                            }
                            "chat" => {
                                // For the chat example, we'll print the message to the console
                                // In a real app, this might be broadcast to other clients
                                if let Some(content) = message.payload.as_str() {
                                    println!("\r[CHAT] {}: {}", message.source_id, content);
                                    print!("> ");
                                    let _ = std::io::Write::flush(&mut std::io::stdout());
                                }
                                
                                let response = CommunicationMessage::response("Message received".to_string());
                                let response_json = serde_json::to_string(&response)?;
                                stream.write_all(response_json.as_bytes()).await?;
                            }
                            _ => {
                                let error = CommunicationMessage::error(
                                    "Unsupported message type".to_string(),
                                );
                                let error_json = serde_json::to_string(&error)?;
                                stream.write_all(error_json.as_bytes()).await?;
                            }
                        },
                        Err(e) => {
                            let error = CommunicationMessage::error(format!(
                                "Failed to parse message: {}",
                                e
                            ));
                            let error_json = serde_json::to_string(&error)?;
                            stream.write_all(error_json.as_bytes()).await?;
                        }
                    }
                }
                Ok(Err(e)) => {
                    eprintln!("Error reading from client: {}", e);
                    break;
                }
                Err(_) => {
                    // Timeout reading from client, just break and let them reconnect if needed
                    break;
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

        // Spawn a task to handle incoming connections
        tokio::spawn(async move {
            while *is_running.lock().await {
                match listener.lock().await.as_ref().unwrap().accept().await {
                    Ok((stream, _addr)) => {
                        let stream_clone = stream;
                        tokio::spawn(async move {
                            if let Err(e) = Self::handle_client(
                                &Self {
                                    config: CommunicationConfig::default(),
                                    listener: Arc::new(Mutex::new(None)),
                                    is_running: Arc::new(Mutex::new(true)),
                                },
                                stream_clone,
                            )
                            .await
                            {
                                eprintln!("Error handling client: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        if *is_running.lock().await {
                            eprintln!("Error accepting connection: {}", e);
                        }
                        break;
                    }
                }
            }

            // Clean up socket file when server stops
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
}

/// Unix Domain Socket client implementation
#[derive(Debug)]
pub struct UnixSocketClient {
    config: CommunicationConfig,
    stream: Option<tokio::net::UnixStream>,
    connected: bool,
}

impl UnixSocketClient {
    pub fn new(config: &CommunicationConfig) -> Result<Self, CommunicationError> {
        let _socket_path = format!("/tmp/{}.sock", config.identifier);

        Ok(Self {
            config: config.clone(),
            stream: None,
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
        self.stream = Some(stream);
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

        let stream = self.stream.as_mut().unwrap();
        let message_json = serde_json::to_string(message)?;
        stream.write_all(message_json.as_bytes()).await?;
        Ok(())
    }

    async fn receive_message(&mut self) -> Result<CommunicationMessage, CommunicationError> {
        if !self.connected {
            return Err(CommunicationError::ConnectionFailed(
                "Not connected".to_string(),
            ));
        }

        let stream = self.stream.as_mut().unwrap();
        let mut buffer = vec![0u8; 4096];
        let bytes_read = stream.read(&mut buffer).await?;

        let message_str = String::from_utf8_lossy(&buffer[..bytes_read]);
        let message: CommunicationMessage = serde_json::from_str(&message_str)
            .map_err(|e| CommunicationError::DeserializationFailed(e.to_string()))?;

        Ok(message)
    }

    async fn disconnect(&mut self) -> Result<(), CommunicationError> {
        self.stream = None;
        self.connected = false;
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected
    }
}
