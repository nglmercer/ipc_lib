//! Request/response broker for correlating async IPC messages.
//!
//! [`RequestBroker`] keeps a map of pending request IDs to one-shot senders so
//! that a caller can `await` a specific response without blocking the shared
//! receive loop.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex as TokioMutex};

use crate::communication::CommunicationMessage;

/// Manages pending requests and routes incoming responses to their waiters.
pub struct RequestBroker {
    pending: Arc<TokioMutex<HashMap<String, oneshot::Sender<CommunicationMessage>>>>,
}

impl Default for RequestBroker {
    fn default() -> Self {
        Self::new()
    }
}

impl RequestBroker {
    /// Create a new, empty `RequestBroker`.
    pub fn new() -> Self {
        Self {
            pending: Arc::new(TokioMutex::new(HashMap::new())),
        }
    }

    /// Register a new request by ID and return a receiver that resolves when
    /// the matching response arrives.
    pub async fn register_request(&self, id: String) -> oneshot::Receiver<CommunicationMessage> {
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        rx
    }

    /// Try to deliver `message` to a waiting caller.
    ///
    /// Returns `true` if `message.reply_to` matched a pending request and the
    /// message was dispatched; `false` otherwise (i.e. it's an unsolicited
    /// message).
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
