// C ABI exports for Bun FFI
pub mod c_abi;

use std::sync::Arc;

use tokio::sync::Mutex;
use uniffi;

uniffi::setup_scaffolding!();

// Error definition
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum CommunicationError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),
    #[error("Serialization failed: {0}")]
    SerializationFailed(String),
    #[error("Deserialization failed: {0}")]
    DeserializationFailed(String),
    #[error("Protocol not supported: {0}")]
    ProtocolNotSupported(String),
    #[error("Timeout: {0}")]
    Timeout(String),
    #[error("Resource not found: {0}")]
    ResourceNotFound(String),
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    #[error("I/O error: {0}")]
    IoError(String),
    #[error("Unknown error: {0}")]
    Unknown(String),
}

impl From<single_instance_app::communication::CommunicationError> for CommunicationError {
    fn from(e: single_instance_app::communication::CommunicationError) -> Self {
        use single_instance_app::communication::CommunicationError as Inner;
        match e {
            Inner::ConnectionFailed(m) => Self::ConnectionFailed(m),
            Inner::SerializationFailed(m) => Self::SerializationFailed(m),
            Inner::DeserializationFailed(m) => Self::DeserializationFailed(m),
            Inner::ProtocolNotSupported(m) => Self::ProtocolNotSupported(m),
            Inner::Timeout(m) => Self::Timeout(m),
            Inner::ResourceNotFound(m) => Self::ResourceNotFound(m),
            Inner::PermissionDenied(m) => Self::PermissionDenied(m),
            Inner::IoError(m) => Self::IoError(m),
        }
    }
}

// Protocol Enum
#[derive(Debug, Clone, uniffi::Enum)]
pub enum ProtocolType {
    UnixSocket,
    NamedPipe,
    SharedMemory,
    FileBased,
    InMemory,
}

impl From<ProtocolType> for single_instance_app::ProtocolType {
    fn from(p: ProtocolType) -> Self {
        match p {
            ProtocolType::UnixSocket => single_instance_app::ProtocolType::UnixSocket,
            ProtocolType::NamedPipe => single_instance_app::ProtocolType::NamedPipe,
            ProtocolType::SharedMemory => single_instance_app::ProtocolType::SharedMemory,
            ProtocolType::FileBased => single_instance_app::ProtocolType::FileBased,
            ProtocolType::InMemory => single_instance_app::ProtocolType::InMemory,
        }
    }
}

// Message Record
#[derive(Debug, Clone, uniffi::Record)]
pub struct CommunicationMessage {
    pub id: String,
    pub message_type: String,
    pub payload_json: String,
    pub timestamp: u64,
    pub source_id: String,
    pub reply_to: Option<String>,
    pub metadata_json: String,
}

impl From<single_instance_app::communication::CommunicationMessage> for CommunicationMessage {
    fn from(msg: single_instance_app::communication::CommunicationMessage) -> Self {
        Self {
            id: msg.id,
            message_type: msg.message_type,
            payload_json: msg.payload.to_string(),
            timestamp: msg.timestamp,
            source_id: msg.source_id,
            reply_to: msg.reply_to,
            metadata_json: msg.metadata.to_string(),
        }
    }
}

impl CommunicationMessage {
    pub fn to_inner(&self) -> Result<single_instance_app::communication::CommunicationMessage, String> {
        let payload: serde_json::Value = serde_json::from_str(&self.payload_json)
            .map_err(|e| format!("Invalid JSON payload: {}", e))?;
        let metadata: serde_json::Value = serde_json::from_str(&self.metadata_json)
            .map_err(|e| format!("Invalid JSON metadata: {}", e))?;
            
        Ok(single_instance_app::communication::CommunicationMessage {
            id: self.id.clone(),
            message_type: self.message_type.clone(),
            payload,
            timestamp: self.timestamp,
            source_id: self.source_id.clone(),
            reply_to: self.reply_to.clone(),
            metadata,
        })
    }
}

// Callback Interface
#[uniffi::export(callback_interface)]
pub trait MessageHandler: Send + Sync {
    fn on_message(&self, message: CommunicationMessage) -> Option<CommunicationMessage>;
}

// Main Object
#[derive(uniffi::Object)]
pub struct SingleInstanceApp {
    inner: Mutex<Option<single_instance_app::SingleInstanceApp>>,
}

#[uniffi::export]
impl SingleInstanceApp {
    #[uniffi::constructor]
    pub fn new(identifier: String) -> Self {
        Self {
            inner: Mutex::new(Some(single_instance_app::SingleInstanceApp::new(&identifier))),
        }
    }

    pub async fn with_protocol(&self, protocol: ProtocolType) -> Result<(), CommunicationError> {
        let mut guard = self.inner.lock().await;
        if let Some(app) = guard.take() {
            *guard = Some(app.with_protocol(protocol.into()));
            Ok(())
        } else {
             Err(CommunicationError::Unknown("App instance invalid".to_string()))
        }
    }

    pub async fn on_message(&self, handler: Box<dyn MessageHandler>) -> Result<(), CommunicationError> {
        let mut guard = self.inner.lock().await;
        if let Some(app) = guard.take() {
             let handler = Arc::new(handler);
             // Create a closure that calls the foreign handler
             let callback = move |msg: single_instance_app::communication::CommunicationMessage| {
                 let outer_msg = CommunicationMessage::from(msg);
                 let response = handler.on_message(outer_msg);
                 
                 match response {
                     Some(r) => r.to_inner().ok(), // If conversion fails, duplicate handling sucks but return None
                     None => None,
                 }
             };
            *guard = Some(app.on_message(callback));
            Ok(())
        } else {
             Err(CommunicationError::Unknown("App instance invalid".to_string()))
        }
    }

    pub async fn enforce_single_instance(&self) -> Result<bool, CommunicationError> {
        let mut guard = self.inner.lock().await;
        if let Some(app) = guard.as_mut() {
            app.enforce_single_instance().await.map_err(CommunicationError::from)
        } else {
            Err(CommunicationError::Unknown("App instance invalid".to_string()))
        }
    }
    
    pub async fn broadcast(&self, message_type: String, payload_json: String) -> Result<(), CommunicationError> {
        let guard = self.inner.lock().await;
        if let Some(app) = guard.as_ref() {
            let payload: serde_json::Value = serde_json::from_str(&payload_json)
                .map_err(|e| CommunicationError::SerializationFailed(e.to_string()))?;
                
            let msg = single_instance_app::communication::CommunicationMessage::new(&message_type, payload);
            app.broadcast(msg).await.map_err(CommunicationError::from)
        } else {
            Err(CommunicationError::Unknown("App instance invalid".to_string()))
        }
    }
}
