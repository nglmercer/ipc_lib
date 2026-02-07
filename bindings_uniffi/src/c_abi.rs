// C ABI wrapper for Bun FFI
// This provides a simple C interface that can be called from Bun's FFI

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::ptr;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::time::{timeout, Duration};

// Opaque handle for SingleInstanceApp
pub struct AppHandle {
    app: single_instance_app::SingleInstanceApp,
    runtime: Arc<Runtime>,
    // Message queue for receiving messages from clients
    messages: Arc<std::sync::Mutex<Vec<single_instance_app::communication::CommunicationMessage>>>,
}

// Opaque handle for IPC Client
pub struct ClientHandle {
    config: single_instance_app::communication::CommunicationConfig,
    runtime: Arc<Runtime>,
    // Persistent connection for receiving messages
    client: Option<Box<dyn single_instance_app::communication::CommunicationClient>>,
}

/// Create a new SingleInstanceApp
/// Returns a pointer to the app handle, or null on error
#[no_mangle]
pub extern "C" fn ipc_app_new(identifier: *const c_char) -> *mut AppHandle {
    if identifier.is_null() {
        return ptr::null_mut();
    }

    let identifier = unsafe {
        match CStr::from_ptr(identifier).to_str() {
            Ok(s) => s,
            Err(_) => return ptr::null_mut(),
        }
    };

    let runtime = match Runtime::new() {
        Ok(rt) => Arc::new(rt),
        Err(_) => return ptr::null_mut(),
    };

    let messages = Arc::new(std::sync::Mutex::new(Vec::new()));
    let messages_clone = messages.clone();
    
    let app = single_instance_app::SingleInstanceApp::new(identifier)
        .on_message(move |msg| {
            // Store received messages in the queue
            messages_clone.lock().unwrap().push(msg.clone());
            // Return a response to acknowledge receipt
            Some(msg.create_reply(serde_json::json!("received")))
        });

    Box::into_raw(Box::new(AppHandle {
        app,
        runtime,
        messages,
    }))
}

/// Enforce single instance
/// Returns 1 if this is the primary instance, 0 if secondary, -1 on error
#[no_mangle]
pub extern "C" fn ipc_app_enforce_single_instance(handle: *mut AppHandle) -> i32 {
    if handle.is_null() {
        return -1;
    }

    let handle = unsafe { &mut *handle };

    match handle.runtime.block_on(handle.app.enforce_single_instance()) {
        Ok(true) => 1,   // Primary instance
        Ok(false) => 0,  // Secondary instance
        Err(_) => -1,    // Error
    }
}

/// Set the protocol type
/// protocol: 0=UnixSocket, 1=NamedPipe, 2=SharedMemory, 3=FileBased, 4=InMemory
/// Returns 0 on success, -1 on error
#[no_mangle]
pub extern "C" fn ipc_app_set_protocol(handle: *mut AppHandle, protocol: i32) -> i32 {
    if handle.is_null() {
        return -1;
    }

    let protocol_type = match protocol {
        0 => single_instance_app::ProtocolType::UnixSocket,
        1 => single_instance_app::ProtocolType::NamedPipe,
        2 => single_instance_app::ProtocolType::SharedMemory,
        3 => single_instance_app::ProtocolType::FileBased,
        4 => single_instance_app::ProtocolType::InMemory,
        _ => return -1,
    };

    let handle = unsafe { &mut *handle };
    handle.app = std::mem::replace(
        &mut handle.app,
        single_instance_app::SingleInstanceApp::new("temp")
    ).with_protocol(protocol_type);

    0
}

/// Broadcast a message to all connected clients
/// Returns 0 on success, -1 on error
#[no_mangle]
pub extern "C" fn ipc_app_broadcast(
    handle: *mut AppHandle,
    message_type: *const c_char,
    payload_json: *const c_char,
) -> i32 {
    if handle.is_null() || message_type.is_null() || payload_json.is_null() {
        return -1;
    }

    let handle = unsafe { &*handle };

    let message_type = unsafe {
        match CStr::from_ptr(message_type).to_str() {
            Ok(s) => s,
            Err(_) => return -1,
        }
    };

    let payload_json = unsafe {
        match CStr::from_ptr(payload_json).to_str() {
            Ok(s) => s,
            Err(_) => return -1,
        }
    };

    let payload: serde_json::Value = match serde_json::from_str(payload_json) {
        Ok(p) => p,
        Err(_) => return -1,
    };

    let msg = single_instance_app::communication::CommunicationMessage::new(message_type, payload);

    match handle.runtime.block_on(handle.app.broadcast(msg)) {
        Ok(_) => 0,
        Err(_) => -1,
    }
}

/// Receive a message from clients (non-blocking, returns null if no message available)
/// Returns pointer to JSON string of the message, or null if no message/error
/// Caller must call ipc_app_free_string to free the returned string
#[no_mangle]
pub extern "C" fn ipc_app_receive(handle: *mut AppHandle) -> *mut c_char {
    if handle.is_null() {
        return ptr::null_mut();
    }

    let handle = unsafe { &mut *handle };

    // Try to pop a message from the queue
    let message = handle.messages.lock().unwrap().pop();

    match message {
        Some(msg) => {
            match serde_json::to_string(&msg) {
                Ok(json) => {
                    let c_string = CString::new(json).unwrap();
                    c_string.into_raw()
                }
                Err(_) => ptr::null_mut() as *mut c_char,
            }
        }
        None => ptr::null_mut() as *mut c_char,
    }
}

/// Create a new IPC client for the given identifier
/// Returns a pointer to the client handle, or null on error
#[no_mangle]
pub extern "C" fn ipc_client_new(identifier: *const c_char) -> *mut ClientHandle {
    if identifier.is_null() {
        return ptr::null_mut();
    }

    let identifier = unsafe {
        match CStr::from_ptr(identifier).to_str() {
            Ok(s) => s,
            Err(_) => return ptr::null_mut(),
        }
    };

    let runtime = match Runtime::new() {
        Ok(rt) => Arc::new(rt),
        Err(_) => return ptr::null_mut(),
    };

    let config = single_instance_app::communication::CommunicationConfig {
        identifier: identifier.to_string(),
        protocol: single_instance_app::ProtocolType::UnixSocket,
        ..Default::default()
    };

    Box::into_raw(Box::new(ClientHandle {
        config,
        runtime,
        client: None,
    }))
}

/// Set protocol for client
/// protocol: 0=UnixSocket, 1=NamedPipe, 2=SharedMemory, 3=FileBased, 4=InMemory
/// Returns 0 on success, -1 on error
#[no_mangle]
pub extern "C" fn ipc_client_set_protocol(handle: *mut ClientHandle, protocol: i32) -> i32 {
    if handle.is_null() {
        return -1;
    }

    let protocol_type = match protocol {
        0 => single_instance_app::ProtocolType::UnixSocket,
        1 => single_instance_app::ProtocolType::NamedPipe,
        2 => single_instance_app::ProtocolType::SharedMemory,
        3 => single_instance_app::ProtocolType::FileBased,
        4 => single_instance_app::ProtocolType::InMemory,
        _ => return -1,
    };

    let handle = unsafe { &mut *handle };
    handle.config.protocol = protocol_type;

    0
}

/// Send a message from client to server
/// Returns 0 on success, -1 on error
#[no_mangle]
pub extern "C" fn ipc_client_send(
    handle: *mut ClientHandle,
    message_type: *const c_char,
    payload_json: *const c_char,
) -> i32 {
    if handle.is_null() || message_type.is_null() || payload_json.is_null() {
        return -1;
    }

    let handle = unsafe { &mut *handle };

    let message_type = unsafe {
        match CStr::from_ptr(message_type).to_str() {
            Ok(s) => s,
            Err(_) => return -1,
        }
    };

    let payload_json = unsafe {
        match CStr::from_ptr(payload_json).to_str() {
            Ok(s) => s,
            Err(_) => return -1,
        }
    };

    let payload: serde_json::Value = match serde_json::from_str(payload_json) {
        Ok(p) => p,
        Err(_) => return -1,
    };

    let runtime = handle.runtime.clone();
    let config = handle.config.clone();

    let result = runtime.block_on(async {
        let protocol = match single_instance_app::communication::CommunicationFactory::create_protocol(config.protocol) {
            Ok(p) => p,
            Err(_) => return -1i32,
        };

        let mut client = match protocol.create_client(&config).await {
            Ok(c) => c,
            Err(_) => return -1i32,
        };

        if let Err(_) = client.connect().await {
            return -1i32;
        }

        let msg = single_instance_app::communication::CommunicationMessage::new(message_type, payload);

        if let Err(_) = client.send_message(&msg).await {
            let _ = client.disconnect().await;
            return -1i32;
        }

        let _ = client.disconnect().await;
        0i32
    });

    result
}

/// Receive the last message (non-blocking, returns null if no message available)
/// Maintains a persistent connection to the server
/// Returns pointer to JSON string of the message, or null if no message/error
/// Caller must call ipc_app_free_string to free the returned string
#[no_mangle]
pub extern "C" fn ipc_client_receive(handle: *mut ClientHandle) -> *mut c_char {
    if handle.is_null() {
        return ptr::null_mut();
    }

    let handle = unsafe { &mut *handle };

    let runtime = handle.runtime.clone();
    let config = handle.config.clone();

    // Try to receive a message using persistent connection
    let result = runtime.block_on(async {
        // Create or reuse client connection
        if handle.client.is_none() {
            let protocol = match single_instance_app::communication::CommunicationFactory::create_protocol(config.protocol) {
                Ok(p) => p,
                Err(_) => return ptr::null_mut() as *mut c_char,
            };

            let mut client = match protocol.create_client(&config).await {
                Ok(c) => c,
                Err(_) => return ptr::null_mut() as *mut c_char,
            };

            if let Err(_) = client.connect().await {
                return ptr::null_mut() as *mut c_char;
            }

            handle.client = Some(client);
        }

        let client = handle.client.as_mut().unwrap();

        // Try to receive a message with a short timeout (non-blocking)
        let receive_result = timeout(Duration::from_millis(50), client.receive_message()).await;

        match receive_result {
            Ok(Ok(msg)) => {
                // Return JSON string of message
                match serde_json::to_string(&msg) {
                    Ok(json) => {
                        let c_string = CString::new(json).unwrap();
                        c_string.into_raw()
                    }
                    Err(_) => ptr::null_mut() as *mut c_char,
                }
            }
            Ok(Err(_)) | Err(_) => {
                // No message available or timeout - don't disconnect, just return null
                ptr::null_mut() as *mut c_char
            }
        }
    });

    result
}

/// Disconnect the client and cleanup persistent connection
#[no_mangle]
pub extern "C" fn ipc_client_disconnect(handle: *mut ClientHandle) -> i32 {
    if handle.is_null() {
        return -1;
    }

    let handle = unsafe { &mut *handle };

    let runtime = handle.runtime.clone();

    runtime.block_on(async {
        if let Some(mut client) = handle.client.take() {
            let _ = client.disconnect().await;
        }
        0i32
    })
}

/// Check if client can connect to server
/// Returns 1 if connection successful, 0 if no server, -1 on error
#[no_mangle]
pub extern "C" fn ipc_client_ping(handle: *mut ClientHandle) -> i32 {
    if handle.is_null() {
        return -1;
    }

    let handle = unsafe { &mut *handle };

    let runtime = handle.runtime.clone();
    let config = handle.config.clone();

    runtime.block_on(async {
        let protocol = match single_instance_app::communication::CommunicationFactory::create_protocol(config.protocol) {
            Ok(p) => p,
            Err(_) => return -1i32,
        };

        let mut client = match protocol.create_client(&config).await {
            Ok(c) => c,
            Err(_) => return 0i32,
        };

        if let Err(_) = client.connect().await {
            return 0i32;
        }

        let _ = client.disconnect().await;
        1i32
    })
}

/// Get the last error message
/// Returns a pointer to the error string, or null if no error
/// Caller must call ipc_app_free_string to free the returned string
#[no_mangle]
pub extern "C" fn ipc_app_get_last_error() -> *mut c_char {
    // This is a simplified error handling for demo purposes
    // In production, you'd want thread-local storage or per-handle errors
    let error_msg = CString::new("Error occurred").unwrap();
    error_msg.into_raw()
}

/// Free a string returned by the FFI
#[no_mangle]
pub extern "C" fn ipc_app_free_string(s: *mut c_char) {
    if !s.is_null() {
        unsafe {
            let _ = CString::from_raw(s);
        }
    }
}

/// Free the app handle
#[no_mangle]
pub extern "C" fn ipc_app_free(handle: *mut AppHandle) {
    if !handle.is_null() {
        unsafe {
            let _ = Box::from_raw(handle);
        }
    }
}

/// Free the client handle
#[no_mangle]
pub extern "C" fn ipc_client_free(handle: *mut ClientHandle) {
    if !handle.is_null() {
        unsafe {
            let _ = Box::from_raw(handle);
        }
    }
}

/// Free an error string returned by ipc_app_get_last_error
#[no_mangle]
pub extern "C" fn ipc_app_free_error(error: *mut c_char) {
    if !error.is_null() {
        unsafe {
            let _ = CString::from_raw(error);
        }
    }
}
