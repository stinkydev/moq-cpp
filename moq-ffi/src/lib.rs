use once_cell::sync::Lazy;
use std::ffi::CStr;
use std::os::raw::c_char;
use std::ptr;
use tokio::runtime::Runtime;

use moq_native::client::{Client, ClientConfig};
use url::Url;

// Global runtime for async operations
static RUNTIME: Lazy<Runtime> =
    Lazy::new(|| Runtime::new().expect("Failed to create tokio runtime"));

/// Opaque handle for the MOQ client
#[repr(C)]
pub struct MoqClient {
    // For now, just a placeholder
    _placeholder: u64,
}

/// Opaque handle for a MOQ session
#[repr(C)]
pub struct MoqSession {
    // Keep it simple for now - just store a connection ID or status
    connection_id: u64,
    is_connected: bool,
}

/// Result codes for MOQ operations
#[repr(C)]
#[derive(Debug, PartialEq)]
pub enum MoqResult {
    Success = 0,
    InvalidArgument = 1,
    NetworkError = 2,
    TlsError = 3,
    DnsError = 4,
    GeneralError = 5,
}

/// Configuration for MOQ client
#[repr(C)]
pub struct MoqClientConfig {
    pub bind_addr: *const c_char,
    pub tls_disable_verify: bool,
    pub tls_root_cert_path: *const c_char,
}

/// Initialize the MOQ FFI library
/// This should be called once before using any other functions
#[no_mangle]
pub extern "C" fn moq_init() -> MoqResult {
    // Initialize tracing for logging
    if let Err(_) = tracing_subscriber::fmt::try_init() {
        // Already initialized, that's fine
    }
    MoqResult::Success
}

/// Create a new MOQ client with the given configuration
///
/// # Arguments
/// * `config` - Configuration for the client
/// * `client_out` - Output parameter for the created client handle
///
/// # Returns
/// * `MoqResult` indicating success or failure
#[no_mangle]
pub extern "C" fn moq_client_new(
    config: *const MoqClientConfig,
    client_out: *mut *mut MoqClient,
) -> MoqResult {
    if config.is_null() || client_out.is_null() {
        return MoqResult::InvalidArgument;
    }

    let config = unsafe { &*config };

    // Convert C strings to Rust strings
    let bind_addr = if config.bind_addr.is_null() {
        "[::]:0"
    } else {
        match unsafe { CStr::from_ptr(config.bind_addr) }.to_str() {
            Ok(addr) => addr,
            Err(_) => return MoqResult::InvalidArgument,
        }
    };

    let mut client_config = ClientConfig::default();

    // Parse bind address
    client_config.bind = match bind_addr.parse() {
        Ok(addr) => addr,
        Err(_) => return MoqResult::InvalidArgument,
    };

    // Set TLS options
    client_config.tls.disable_verify = if config.tls_disable_verify {
        Some(true)
    } else {
        None
    };

    // Set root certificate if provided
    if !config.tls_root_cert_path.is_null() {
        let root_path = match unsafe { CStr::from_ptr(config.tls_root_cert_path) }.to_str() {
            Ok(path) => path,
            Err(_) => return MoqResult::InvalidArgument,
        };
        client_config.tls.root = vec![root_path.into()];
    }

    // For now, just create a dummy client
    let boxed_client = Box::new(MoqClient {
        _placeholder: 12345,
    });
    unsafe {
        *client_out = Box::into_raw(boxed_client);
    }

    MoqResult::Success
}

/// Connect to a MOQ server
///
/// # Arguments
/// * `client` - Handle to the MOQ client
/// * `url` - URL to connect to (as C string)
/// * `session_out` - Output parameter for the created session handle
///
/// # Returns
/// * `MoqResult` indicating success or failure
#[no_mangle]
pub extern "C" fn moq_client_connect(
    client: *mut MoqClient,
    url: *const c_char,
    session_out: *mut *mut MoqSession,
) -> MoqResult {
    if client.is_null() || url.is_null() || session_out.is_null() {
        return MoqResult::InvalidArgument;
    }

    let _client = unsafe { &*client };

    let _url_str = match unsafe { CStr::from_ptr(url) }.to_str() {
        Ok(url) => url,
        Err(_) => return MoqResult::InvalidArgument,
    };

    // Create a dummy session for now
    let boxed_session = Box::new(MoqSession {
        connection_id: 42,
        is_connected: true,
    });
    unsafe {
        *session_out = Box::into_raw(boxed_session);
    }

    MoqResult::Success
}

/// Free a MOQ client handle
///
/// # Arguments
/// * `client` - Handle to the MOQ client to free
#[no_mangle]
pub extern "C" fn moq_client_free(client: *mut MoqClient) {
    if !client.is_null() {
        unsafe {
            let _ = Box::from_raw(client);
        }
    }
}

/// Free a MOQ session handle
///
/// # Arguments
/// * `session` - Handle to the MOQ session to free
#[no_mangle]
pub extern "C" fn moq_session_free(session: *mut MoqSession) {
    if !session.is_null() {
        unsafe {
            let _ = Box::from_raw(session);
        }
    }
}

/// Check if a session is connected
///
/// # Arguments
/// * `session` - Handle to the MOQ session
///
/// # Returns
/// * true if connected, false otherwise
#[no_mangle]
pub extern "C" fn moq_session_is_connected(session: *const MoqSession) -> bool {
    if session.is_null() {
        return false;
    }

    unsafe { (*session).is_connected }
}

/// Close a MOQ session
///
/// # Arguments
/// * `session` - Handle to the MOQ session to close
#[no_mangle]
pub extern "C" fn moq_session_close(session: *mut MoqSession) -> MoqResult {
    if session.is_null() {
        return MoqResult::InvalidArgument;
    }

    unsafe {
        (*session).is_connected = false;
    }

    MoqResult::Success
}

/// Get the last error message (thread-local)
///
/// # Returns
/// * Pointer to a null-terminated string containing the error message
/// * The returned string is valid until the next call to any MOQ function
/// * Returns null if no error occurred
#[no_mangle]
pub extern "C" fn moq_get_last_error() -> *const c_char {
    // For now, return null - in a real implementation you'd want proper error handling
    ptr::null()
}

/// Convert a MoqResult to a human-readable string
///
/// # Arguments
/// * `result` - The result code to convert
///
/// # Returns
/// * Pointer to a null-terminated string describing the result
#[no_mangle]
pub extern "C" fn moq_result_to_string(result: MoqResult) -> *const c_char {
    let str = match result {
        MoqResult::Success => "Success\0",
        MoqResult::InvalidArgument => "Invalid argument\0",
        MoqResult::NetworkError => "Network error\0",
        MoqResult::TlsError => "TLS error\0",
        MoqResult::DnsError => "DNS error\0",
        MoqResult::GeneralError => "General error\0",
    };
    str.as_ptr() as *const c_char
}
