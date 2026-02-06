//! In-memory communication protocol implementation
//! Provides IPC communication using shared memory for testing
//! Doesn't require any file system or network access

use super::*;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};

/// In-memory communication protocol implementation
#[derive(Debug)]
pub struct InMemoryProtocol;

#[async_trait::async_trait]
impl CommunicationProtocol for InMemoryProtocol {
    fn protocol_type(&self) -> ProtocolType {
        ProtocolType::InMemory
    }

    async fn create_server(
        &self,
        config: &CommunicationConfig,
    ) -> Result<Box<dyn CommunicationServer>, CommunicationError> {
        Ok(Box::new(InMemoryServer::new(config)?))
    }

    async fn create_client(
        &self,
        config: &CommunicationConfig,
    ) -> Result<Box<dyn CommunicationClient>, CommunicationError> {
        Ok(Box::new(InMemoryClient::new(config)?))
    }
}

/// In-memory server implementation
#[derive(Debug)]
pub struct InMemoryServer {
    config: CommunicationConfig,
    message_sender: Arc<Mutex<Option<mpsc::Sender<CommunicationMessage>>>>,
    response_sender: Arc<Mutex<Option<mpsc::Sender<CommunicationMessage>>>>,
    is_running: Arc<Mutex<bool>>,
}

impl InMemoryServer {
    pub fn new(config: &CommunicationConfig) -> Result<Self, CommunicationError> {
        Ok(Self {
            config: config.clone(),
            message_sender: Arc::new(Mutex::new(None)),
            response_sender: Arc::new(Mutex::new(None)),
            is_running: Arc::new(Mutex::new(false)),
        })
    }

    async fn handle_message(
        &self,
        message: CommunicationMessage,
    ) -> Result<CommunicationMessage, CommunicationError> {
        match message.message_type.as_str() {
            "command_line_args" => {
                let response = CommunicationMessage::response(
                    "Command line arguments received successfully".to_string(),
                );
                Ok(response)
            }
            _ => {
                let error = CommunicationMessage::error("Unsupported message type".to_string());
                Ok(error)
            }
        }
    }
}

#[async_trait::async_trait]
impl CommunicationServer for InMemoryServer {
    async fn start(&mut self) -> Result<(), CommunicationError> {
        // Create channels for communication
        let (message_sender, message_receiver) = mpsc::channel(10);
        let (response_sender, _) = mpsc::channel(10);

        // Clone the response sender for storing in self
        let response_sender_clone = response_sender.clone();

        // Store the senders
        *self.message_sender.lock().await = Some(message_sender);
        *self.response_sender.lock().await = Some(response_sender_clone);

        *self.is_running.lock().await = true;

        // Spawn a task to handle incoming messages
        let is_running = self.is_running.clone();
        let message_receiver = Arc::new(Mutex::new(Some(message_receiver)));
        let response_sender = Arc::new(Mutex::new(Some(response_sender)));

        tokio::spawn(async move {
            while *is_running.lock().await {
                // Receive message
                let receiver_opt = message_receiver.lock().await.take();
                if let Some(mut receiver) = receiver_opt {
                    match timeout(Duration::from_millis(100), receiver.recv()).await {
                        Ok(Some(message)) => {
                            // Handle the message and send response
                            let response = Self::handle_message(
                                &Self {
                                    config: CommunicationConfig::default(),
                                    message_sender: Arc::new(Mutex::new(None)),
                                    response_sender: Arc::new(Mutex::new(None)),
                                    is_running: is_running.clone(),
                                },
                                message,
                            )
                            .await;

                            if let Ok(response) = response {
                                let sender_opt = response_sender.lock().await.take();
                                if let Some(sender) = sender_opt {
                                    let _ = sender.send(response).await;
                                }
                            }
                        }
                        Ok(None) => {
                            // Channel closed
                            break;
                        }
                        Err(_) => {
                            // Timeout, continue loop
                            continue;
                        }
                    }
                }
            }
        });

        Ok(())
    }

    async fn stop(&mut self) -> Result<(), CommunicationError> {
        *self.is_running.lock().await = false;

        // Close channels
        *self.message_sender.lock().await = None;
        *self.response_sender.lock().await = None;

        Ok(())
    }

    fn is_running(&self) -> bool {
        *self.is_running.blocking_lock()
    }

    fn endpoint(&self) -> String {
        format!("in-memory://{}", self.config.identifier)
    }
}

/// In-memory client implementation
#[derive(Debug)]
pub struct InMemoryClient {
    config: CommunicationConfig,
    message_sender: Arc<Mutex<Option<mpsc::Sender<CommunicationMessage>>>>,
    response_receiver: Arc<Mutex<Option<mpsc::Receiver<CommunicationMessage>>>>,
    connected: bool,
}

impl InMemoryClient {
    pub fn new(config: &CommunicationConfig) -> Result<Self, CommunicationError> {
        Ok(Self {
            config: config.clone(),
            message_sender: Arc::new(Mutex::new(None)),
            response_receiver: Arc::new(Mutex::new(None)),
            connected: false,
        })
    }

    async fn wait_for_response(&self) -> Result<CommunicationMessage, CommunicationError> {
        let timeout_duration = Duration::from_millis(self.config.timeout_ms);

        let receiver_opt = self.response_receiver.lock().await.take();
        if let Some(mut receiver) = receiver_opt {
            match timeout(timeout_duration, receiver.recv()).await {
                Ok(Some(message)) => Ok(message),
                Ok(None) => Err(CommunicationError::ConnectionFailed(
                    "Server disconnected".to_string(),
                )),
                Err(_) => Err(CommunicationError::Timeout(
                    "No response received".to_string(),
                )),
            }
        } else {
            Err(CommunicationError::ConnectionFailed(
                "Not connected".to_string(),
            ))
        }
    }
}

#[async_trait::async_trait]
impl CommunicationClient for InMemoryClient {
    async fn connect(&mut self) -> Result<(), CommunicationError> {
        // For in-memory communication, we just need to ensure the server is available
        // In a real implementation, this would check if the server exists
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

        let sender_opt = self.message_sender.lock().await.take();
        if let Some(sender) = sender_opt {
            sender
                .send(message.clone())
                .await
                .map_err(|e| CommunicationError::ConnectionFailed(e.to_string()))?;
            Ok(())
        } else {
            Err(CommunicationError::ConnectionFailed(
                "Server not available".to_string(),
            ))
        }
    }

    async fn receive_message(&mut self) -> Result<CommunicationMessage, CommunicationError> {
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
