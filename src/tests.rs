//! Unit and integration tests for the IPC library.

use crate::communication::{
    CommunicationConfig, CommunicationError, CommunicationFactory, CommunicationMessage,
    ProtocolType, SerializationFormat,
};
use crate::single_instance::SingleInstanceApp;
use crate::{client::IpcClient, server::IpcServer, Message};

// ============ SingleInstanceApp Tests ============

#[test]
fn test_ipc_lib_new() {
    let app = SingleInstanceApp::new("test_app");
    assert_eq!(app.config().identifier, "test_app");
}

#[test]
fn test_ipc_lib_with_protocol() {
    let app = SingleInstanceApp::new("test_app")
        .with_protocol(ProtocolType::FileBased)
        .with_timeout(3000);

    assert_eq!(app.config().protocol, ProtocolType::FileBased);
    assert_eq!(app.config().timeout_ms, 3000);
}

#[test]
fn test_ipc_lib_without_fallback() {
    let app = SingleInstanceApp::new("test_app").without_fallback();
    assert!(!app.config().enable_fallback);
}

#[test]
fn test_ipc_lib_with_fallback_protocols() {
    let protocols = vec![ProtocolType::FileBased, ProtocolType::InMemory];
    let app = SingleInstanceApp::new("test_app").with_fallback_protocols(protocols.clone());
    assert_eq!(app.config().fallback_protocols, protocols);
}

#[test]
fn test_ipc_lib_endpoint_none_when_not_started() {
    let app = SingleInstanceApp::new("test_app");
    assert!(app.endpoint().is_none());
}

// ============ IpcClient Tests ============

#[test]
fn test_ipc_client_new() {
    let client = IpcClient::new("test_client");
    assert!(client.is_ok());
    let client = client.unwrap();
    assert_eq!(client.config().identifier, "test_client");
}

#[test]
fn test_ipc_client_default_protocol() {
    let client = IpcClient::new("test_client").unwrap();
    assert_eq!(client.config().protocol, ProtocolType::SharedMemory);
}

// ============ IpcServer Tests ============

#[test]
fn test_ipc_server_new() {
    let server = IpcServer::new("test_server");
    assert!(server.is_ok());
    let server = server.unwrap();
    assert!(server.endpoint().is_none());
}

// ============ Message Tests ============

#[test]
fn test_message_command_line_args() {
    let args = vec!["arg1".to_string(), "arg2".to_string()];
    let message = Message::CommandLineArgs(args.clone());

    match message {
        Message::CommandLineArgs(received_args) => {
            assert_eq!(received_args, args);
        }
        _ => panic!("Expected CommandLineArgs variant"),
    }
}

#[test]
fn test_message_response() {
    let message = Message::Response("test response".to_string());

    match message {
        Message::Response(content) => {
            assert_eq!(content, "test response");
        }
        _ => panic!("Expected Response variant"),
    }
}

#[test]
fn test_message_error() {
    let message = Message::Error("test error".to_string());

    match message {
        Message::Error(error_msg) => {
            assert_eq!(error_msg, "test error");
        }
        _ => panic!("Expected Error variant"),
    }
}

#[test]
fn test_message_serialization() {
    let message = Message::CommandLineArgs(vec!["test".to_string()]);
    let serialized = serde_json::to_string(&message);
    assert!(serialized.is_ok());

    let deserialized: Result<Message, _> = serde_json::from_str(&serialized.unwrap());
    assert!(deserialized.is_ok());
    assert_eq!(deserialized.unwrap(), message);
}

#[test]
fn test_message_debug_format() {
    let message = Message::Response("test".to_string());
    let debug_format = format!("{:?}", message);
    assert!(debug_format.contains("Response"));
    assert!(debug_format.contains("test"));
}

// ============ ProtocolType Tests ============

#[test]
fn test_protocol_type_variants() {
    let _ = ProtocolType::UnixSocket;
    let _ = ProtocolType::NamedPipe;
    let _ = ProtocolType::SharedMemory;
    let _ = ProtocolType::FileBased;
    let _ = ProtocolType::InMemory;
}

#[test]
fn test_protocol_type_debug() {
    assert_eq!(format!("{:?}", ProtocolType::UnixSocket), "UnixSocket");
    assert_eq!(format!("{:?}", ProtocolType::FileBased), "FileBased");
    assert_eq!(format!("{:?}", ProtocolType::InMemory), "InMemory");
}

#[test]
fn test_protocol_type_clone() {
    let protocol = ProtocolType::UnixSocket;
    let cloned = protocol;
    assert_eq!(protocol, cloned);
}

// ============ CommunicationConfig Tests ============

#[test]
fn test_communication_config_default() {
    let config = CommunicationConfig::default();
    assert_eq!(config.protocol, ProtocolType::SharedMemory);
    assert_eq!(config.identifier, "default");
    assert_eq!(config.timeout_ms, 5000);
    assert!(config.enable_fallback);
    assert!(!config.fallback_protocols.is_empty());
}

#[test]
fn test_communication_config_custom() {
    let config = CommunicationConfig {
        protocol: ProtocolType::FileBased,
        serialization_format: SerializationFormat::Json,
        identifier: "custom".to_string(),
        timeout_ms: 10000,
        enable_fallback: false,
        fallback_protocols: vec![],
    };

    assert_eq!(config.protocol, ProtocolType::FileBased);
    assert_eq!(config.identifier, "custom");
    assert_eq!(config.timeout_ms, 10000);
    assert!(!config.enable_fallback);
    assert!(config.fallback_protocols.is_empty());
}

#[test]
fn test_communication_config_debug() {
    let config = CommunicationConfig::default();
    let debug_format = format!("{:?}", config);
    assert!(debug_format.contains("SharedMemory"));
    assert!(debug_format.contains("default"));
}

#[test]
fn test_communication_config_clone() {
    let config = CommunicationConfig::default();
    let cloned = config.clone();
    assert_eq!(config.protocol, cloned.protocol);
    assert_eq!(config.identifier, cloned.identifier);
    assert_eq!(config.timeout_ms, cloned.timeout_ms);
}

// ============ CommunicationMessage Tests ============

#[test]
fn test_communication_message_command_line_args() {
    let args = vec!["--flag".to_string(), "value".to_string()];
    let message = CommunicationMessage::command_line_args(args.clone());

    assert_eq!(message.message_type, "command_line_args");
    assert_eq!(message.source_id, "client");
    assert!(message.timestamp > 0);

    let payload_args: Vec<String> = serde_json::from_value(message.payload).unwrap();
    assert_eq!(payload_args, args);
}

#[test]
fn test_communication_message_response() {
    let message = CommunicationMessage::response("Success!".to_string());

    assert_eq!(message.message_type, "response");
    assert_eq!(message.source_id, "server");
    assert!(message.timestamp > 0);

    let payload_content: String = serde_json::from_value(message.payload).unwrap();
    assert_eq!(payload_content, "Success!");
}

#[test]
fn test_communication_message_error() {
    let message = CommunicationMessage::error("Something went wrong".to_string());

    assert_eq!(message.message_type, "error");
    assert_eq!(message.source_id, "server");

    let payload_error: String = serde_json::from_value(message.payload).unwrap();
    assert_eq!(payload_error, "Something went wrong");
}

#[test]
fn test_communication_message_serialization() {
    let message = CommunicationMessage::response("test".to_string());
    let serialized = serde_json::to_string(&message);
    assert!(serialized.is_ok());

    let deserialized: Result<CommunicationMessage, _> = serde_json::from_str(&serialized.unwrap());
    assert!(deserialized.is_ok());

    let deserialized = deserialized.unwrap();
    assert_eq!(deserialized.message_type, "response");
    let content: String = serde_json::from_value(deserialized.payload).unwrap();
    assert_eq!(content, "test");
}

#[test]
fn test_communication_message_metadata() {
    let message = CommunicationMessage::command_line_args(vec![]);
    assert_eq!(message.metadata, serde_json::json!(null));
}

#[test]
fn test_communication_message_timestamp_ordering() {
    let message1 = CommunicationMessage::command_line_args(vec![]);
    std::thread::sleep(std::time::Duration::from_millis(1));
    let message2 = CommunicationMessage::command_line_args(vec![]);
    assert!(message2.timestamp >= message1.timestamp);
}

// ============ CommunicationError Tests ============

#[test]
fn test_communication_error_display() {
    let error = CommunicationError::ConnectionFailed("test error".to_string());
    let display = format!("{}", error);
    assert!(display.contains("Connection failed"));
    assert!(display.contains("test error"));
}

#[test]
fn test_communication_error_variants() {
    let _ = CommunicationError::ConnectionFailed("test".to_string());
    let _ = CommunicationError::SerializationFailed("test".to_string());
    let _ = CommunicationError::DeserializationFailed("test".to_string());
    let _ = CommunicationError::ProtocolNotSupported("test".to_string());
    let _ = CommunicationError::Timeout("test".to_string());
    let _ = CommunicationError::ResourceNotFound("test".to_string());
    let _ = CommunicationError::PermissionDenied("test".to_string());
    let _ = CommunicationError::IoError("test".to_string());
}

#[test]
fn test_communication_error_source() {
    use std::error::Error;
    let error = CommunicationError::ConnectionFailed("test".to_string());
    let source = error.source();
    assert!(source.is_none());
}

// ============ CommunicationFactory Tests ============

#[test]
fn test_communication_factory_create_protocols() {
    let always_available = vec![ProtocolType::FileBased, ProtocolType::InMemory];

    for protocol in always_available {
        let result = CommunicationFactory::create_protocol(protocol);
        assert!(
            result.is_ok(),
            "Protocol {:?} should be available",
            protocol
        );
    }

    #[cfg(unix)]
    {
        let unix_protocols = vec![ProtocolType::UnixSocket, ProtocolType::SharedMemory];
        for protocol in unix_protocols {
            let result = CommunicationFactory::create_protocol(protocol);
            assert!(
                result.is_ok(),
                "Protocol {:?} should be available on Unix",
                protocol
            );
        }
    }

    #[cfg(windows)]
    {
        let result = CommunicationFactory::create_protocol(ProtocolType::NamedPipe);
        assert!(result.is_err(), "NamedPipe should not be implemented yet");
    }
}

#[test]
fn test_communication_factory_get_available_protocols() {
    let protocols = CommunicationFactory::get_available_protocols();

    assert!(protocols.contains(&ProtocolType::FileBased));
    assert!(protocols.contains(&ProtocolType::InMemory));

    #[cfg(unix)]
    {
        assert!(protocols.contains(&ProtocolType::UnixSocket));
        assert!(protocols.contains(&ProtocolType::SharedMemory));
    }

    #[cfg(windows)]
    {
        assert!(!protocols.contains(&ProtocolType::NamedPipe));
    }
}

// ============ Edge Cases and Integration Tests ============

#[test]
fn test_identifier_length_variations() {
    let app = SingleInstanceApp::new("a");
    assert_eq!(app.config().identifier, "a");

    let long_id = "a".repeat(100);
    let app = SingleInstanceApp::new(&long_id);
    assert_eq!(app.config().identifier, long_id);

    let special_id = "app-with-dots_and_underscores.123";
    let app = SingleInstanceApp::new(special_id);
    assert_eq!(app.config().identifier, special_id);
}

#[test]
fn test_builder_pattern_chaining() {
    let app = SingleInstanceApp::new("test")
        .with_protocol(ProtocolType::FileBased)
        .with_timeout(1000)
        .without_fallback();

    assert_eq!(app.config().protocol, ProtocolType::FileBased);
    assert_eq!(app.config().timeout_ms, 1000);
    assert!(!app.config().enable_fallback);
}

#[test]
fn test_empty_args_message() {
    let message = CommunicationMessage::command_line_args(vec![]);
    let payload_args: Vec<String> = serde_json::from_value(message.payload).unwrap();
    assert!(payload_args.is_empty());
}

#[test]
fn test_multiline_response_message() {
    let multiline = "Line 1\nLine 2\nLine 3";
    let message = CommunicationMessage::response(multiline.to_string());
    let content: String = serde_json::from_value(message.payload).unwrap();
    assert_eq!(content, multiline);
}

#[test]
fn test_special_characters_in_error() {
    let special_error = "Error with 'quotes' and \"double quotes\" and unicode: café";
    let message = CommunicationMessage::error(special_error.to_string());
    let content: String = serde_json::from_value(message.payload).unwrap();
    assert_eq!(content, special_error);
}
