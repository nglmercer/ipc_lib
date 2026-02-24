use crate::communication::error::CommunicationError;
use crate::communication::traits::CommunicationProtocol;
use crate::communication::types::ProtocolType;

// We need to import the local protocol implementations.
// These will still be defined as `mod` in communication/mod.rs or we can move them too.
// For now, let's assume they are sibling modules in communication.
use crate::communication::file_based::FileBasedProtocol;
use crate::communication::in_memory::InMemoryProtocol;
use crate::communication::shared_memory::SharedMemoryProtocol;
use crate::communication::unix_socket::UnixSocketProtocol;

/// Factory for creating communication protocols
pub struct CommunicationFactory;

impl CommunicationFactory {
    /// Create a protocol implementation
    pub fn create_protocol(
        protocol_type: ProtocolType,
    ) -> Result<Box<dyn CommunicationProtocol>, CommunicationError> {
        match protocol_type {
            ProtocolType::UnixSocket => {
                #[cfg(unix)]
                {
                    Ok(Box::new(UnixSocketProtocol))
                }
                #[cfg(not(unix))]
                {
                    Err(CommunicationError::ProtocolNotSupported(
                        "UnixSocket is only supported on Unix-like systems".to_string(),
                    ))
                }
            }
            ProtocolType::SharedMemory => {
                #[cfg(any(unix, windows))]
                {
                    Ok(Box::new(SharedMemoryProtocol))
                }
                #[cfg(not(any(unix, windows)))]
                {
                    Err(CommunicationError::ProtocolNotSupported(
                        "SharedMemory is only supported on Unix-like systems and Windows"
                            .to_string(),
                    ))
                }
            }
            ProtocolType::FileBased => Ok(Box::new(FileBasedProtocol)),
            ProtocolType::InMemory => Ok(Box::new(InMemoryProtocol)),
            ProtocolType::NamedPipe => {
                #[cfg(windows)]
                {
                    // NamedPipe not yet implemented, but could be in future
                    Err(CommunicationError::ProtocolNotSupported(
                        "NamedPipe protocol is not yet implemented".to_string(),
                    ))
                }
                #[cfg(not(windows))]
                {
                    Err(CommunicationError::ProtocolNotSupported(
                        "NamedPipe is only supported on Windows".to_string(),
                    ))
                }
            }
        }
    }

    /// Get available protocols for the current platform
    #[cfg(unix)]
    pub fn get_available_protocols() -> Vec<ProtocolType> {
        vec![
            ProtocolType::UnixSocket,
            ProtocolType::SharedMemory,
            ProtocolType::FileBased,
            ProtocolType::InMemory,
        ]
    }

    /// Get available protocols for the current platform (Windows)
    #[cfg(not(unix))]
    pub fn get_available_protocols() -> Vec<ProtocolType> {
        vec![
            ProtocolType::SharedMemory,
            ProtocolType::FileBased,
            ProtocolType::InMemory,
        ]
    }
}
