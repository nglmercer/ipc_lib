// C ABI wrapper for Bun FFI
// This provides a simple C interface that can be called from Bun's FFI

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::ptr;
use std::sync::Arc;
use tokio::runtime::Runtime;

// Opaque handle for SingleInstanceApp
pub struct AppHandle {
    app: single_instance_app::SingleInstanceApp,
    runtime: Arc<Runtime>,
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

    let app = single_instance_app::SingleInstanceApp::new(identifier);

    Box::into_raw(Box::new(AppHandle {
        app,
        runtime,
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

/// Get the last error message
/// Note: This is a simplified error handling for demo purposes
/// In production, use proper error handling per-thread or per-handle
#[no_mangle]
pub extern "C" fn ipc_app_get_last_error() -> *const c_char {
    // Return a static error message for now
    // In a real implementation, you'd want thread-local storage or per-handle errors
    b"Error occurred\0".as_ptr() as *const c_char
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

/// Free an error string returned by ipc_app_get_last_error
#[no_mangle]
pub extern "C" fn ipc_app_free_error(error: *mut c_char) {
    if !error.is_null() {
        unsafe {
            let _ = CString::from_raw(error);
        }
    }
}
