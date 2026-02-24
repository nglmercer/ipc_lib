//! Communication protocol abstraction layer
//! Provides a unified interface for different IPC communication methods

// Sub-modules for organizing communication logic
mod error;
mod factory;
mod message;
mod traits;
mod types;
mod utils;

// Protocol implementations
mod file_based;
mod in_memory;
mod shared_memory;
mod unix_socket;

// Public re-exports
pub use error::CommunicationError;
pub use factory::CommunicationFactory;
pub use message::{current_timestamp, uuid_v4, CommunicationMessage};
pub use traits::{
    CommunicationClient, CommunicationProtocol, CommunicationServer, MessageHandler,
    SharedMessageHandler,
};
pub use types::{CommunicationConfig, ProtocolType, SerializationFormat};
pub use utils::get_temp_path;

// Protocol implementations re-exports
pub use file_based::FileBasedProtocol;
pub use in_memory::InMemoryProtocol;
pub use shared_memory::SharedMemoryProtocol;
pub use unix_socket::UnixSocketProtocol;
