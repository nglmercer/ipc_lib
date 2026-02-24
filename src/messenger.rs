//! High-level messaging wrapper built on top of [`IpcSession`].
//!
//! [`Messenger`] spawns a background receive loop and uses a [`RequestBroker`]
//! to route responses back to their corresponding `request()` callers.

use std::sync::Arc;

use crate::broker::RequestBroker;
use crate::communication::{CommunicationError, CommunicationMessage};
use crate::session::IpcSession;

/// Wraps an [`IpcSession`] with request/response correlation and fire-and-forget
/// send capabilities.
pub struct Messenger {
    session: Arc<IpcSession>,
    broker: Arc<RequestBroker>,
}

impl Messenger {
    /// Create a new `Messenger` from an already-connected [`IpcSession`].
    ///
    /// A background Tokio task is spawned immediately to receive messages and
    /// route them through the internal [`RequestBroker`].
    pub fn new(session: IpcSession) -> Self {
        let messenger = Self {
            session: Arc::new(session),
            broker: Arc::new(RequestBroker::new()),
        };

        messenger.start_receiver();
        messenger
    }

    /// Spawn the background receive loop.
    fn start_receiver(&self) {
        let session = self.session.clone();
        let broker = self.broker.clone();

        tokio::spawn(async move {
            loop {
                match session.receive().await {
                    Ok(msg) => {
                        if !broker.dispatch_response(msg.clone()).await {
                            // Unsolicited message – a real application would
                            // forward this to a dedicated notification channel.
                            crate::ipc_log!("Unsolicited message received: {:?}", msg.message_type);
                        }
                    }
                    Err(e) => {
                        crate::ipc_log!("Messenger receiver task error: {}", e);
                        break;
                    }
                }
            }
        });
    }

    /// Send a request and wait for the matching response.
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

    /// Send a message without waiting for a response.
    pub async fn send(&self, message: CommunicationMessage) -> Result<(), CommunicationError> {
        self.session.send(message).await
    }
}
