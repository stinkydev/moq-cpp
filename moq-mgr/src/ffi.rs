use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::sync::Arc;

use crate::{Session, SessionConfig, SessionMode, BroadcastConfig, SubscriptionConfig};

/// Result codes for FFI functions
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MoqMgrResult {
    Success = 0,
    ErrorInvalidParameter = -1,
    ErrorNotConnected = -2,
    ErrorAlreadyConnected = -3,
    ErrorInternal = -4,
}

/// Opaque handle to a MoQ Manager session
pub struct MoqMgrSession {
    session: Arc<Session>,
    runtime: Arc<tokio::runtime::Runtime>,
}

/// Error callback function type
/// Parameters: error_message (null-terminated C string), user_data
pub type MoqMgrErrorCallback = extern "C" fn(*const c_char, *mut c_void);

/// Status callback function type
/// Parameters: status_message (null-terminated C string), user_data
pub type MoqMgrStatusCallback = extern "C" fn(*const c_char, *mut c_void);

/// Data callback function type for consumers
/// Parameters: data pointer, data length, user_data
pub type MoqMgrDataCallback = extern "C" fn(*const u8, usize, *mut c_void);

/// Initialize the MoQ Manager library
/// This should be called once at startup
#[no_mangle]
pub extern "C" fn moq_mgr_init() -> MoqMgrResult {
    // Initialize tracing/logging
    tracing_subscriber::fmt::init();
    MoqMgrResult::Success
}

/// Create a new MoQ Manager session
/// 
/// Parameters:
/// - server_url: The MoQ server URL (e.g., "https://relay.moq.example.com:4433")
/// - namespace: The broadcast namespace to use
/// - mode: Session mode (0=PublishOnly, 1=SubscribeOnly, 2=PublishAndSubscribe)
/// - reconnect: Whether to automatically reconnect on failure (0=false, 1=true)
///
/// Returns: Pointer to MoqMgrSession or null on error
#[no_mangle]
pub extern "C" fn moq_mgr_session_create(
    server_url: *const c_char,
    namespace: *const c_char,
    mode: i32,
    reconnect: i32,
) -> *mut MoqMgrSession {
    moq_mgr_session_create_with_bind(server_url, namespace, mode, reconnect, std::ptr::null())
}

/// Create a new MoQ Manager session with custom bind address
/// 
/// Parameters:
/// - server_url: The MoQ server URL (e.g., "https://relay.moq.example.com:4433")
/// - namespace: The broadcast namespace to use
/// - mode: Session mode (0=PublishOnly, 1=SubscribeOnly, 2=PublishAndSubscribe)
/// - reconnect: Whether to automatically reconnect on failure (0=false, 1=true)
/// - bind_addr: Optional bind address (e.g., "0.0.0.0:0" for IPv4, null for default)
///
/// Returns: Pointer to MoqMgrSession or null on error
#[no_mangle]
pub extern "C" fn moq_mgr_session_create_with_bind(
    server_url: *const c_char,
    namespace: *const c_char,
    mode: i32,
    reconnect: i32,
    bind_addr: *const c_char,
) -> *mut MoqMgrSession {
    if server_url.is_null() || namespace.is_null() {
        return std::ptr::null_mut();
    }

    let server_url_str = unsafe {
        match CStr::from_ptr(server_url).to_str() {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        }
    };

    let namespace_str = unsafe {
        match CStr::from_ptr(namespace).to_str() {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        }
    };

    let url = match server_url_str.parse() {
        Ok(u) => u,
        Err(_) => return std::ptr::null_mut(),
    };

    let session_mode = match mode {
        0 => SessionMode::PublishOnly,
        1 => SessionMode::SubscribeOnly,
        2 => SessionMode::PublishAndSubscribe,
        _ => return std::ptr::null_mut(),
    };

    let mut client_config = moq_native::ClientConfig::default();
    
    // Parse bind address if provided
    if !bind_addr.is_null() {
        let bind_addr_str = unsafe {
            match CStr::from_ptr(bind_addr).to_str() {
                Ok(s) => s,
                Err(_) => return std::ptr::null_mut(),
            }
        };
        
        match bind_addr_str.parse() {
            Ok(addr) => client_config.bind = addr,
            Err(_) => return std::ptr::null_mut(),
        }
    }

    let config = SessionConfig {
        moq_server_url: url,
        moq_namespace: namespace_str.to_string(),
        reconnect_on_failure: reconnect != 0,
        client_config,
    };

    let session = Session::new(config, session_mode);
    
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => Arc::new(rt),
        Err(_) => return std::ptr::null_mut(),
    };

    Box::into_raw(Box::new(MoqMgrSession {
        session: Arc::new(session),
        runtime,
    }))
}

/// Set error callback for the session
#[no_mangle]
pub extern "C" fn moq_mgr_session_set_error_callback(
    session: *mut MoqMgrSession,
    callback: MoqMgrErrorCallback,
    user_data: *mut c_void,
) -> MoqMgrResult {
    if session.is_null() {
        return MoqMgrResult::ErrorInvalidParameter;
    }

    let session = unsafe { &*session };
    let user_data_ptr = user_data as usize;
    
    session.session.set_error_callback(move |msg: &str| {
        let c_msg = match CString::new(msg) {
            Ok(s) => s,
            Err(_) => return,
        };
        callback(c_msg.as_ptr(), user_data_ptr as *mut c_void);
    });

    MoqMgrResult::Success
}

/// Set status callback for the session
#[no_mangle]
pub extern "C" fn moq_mgr_session_set_status_callback(
    session: *mut MoqMgrSession,
    callback: MoqMgrStatusCallback,
    user_data: *mut c_void,
) -> MoqMgrResult {
    if session.is_null() {
        return MoqMgrResult::ErrorInvalidParameter;
    }

    let session = unsafe { &*session };
    let user_data_ptr = user_data as usize;
    
    session.session.set_status_callback(move |msg: &str| {
        let c_msg = match CString::new(msg) {
            Ok(s) => s,
            Err(_) => return,
        };
        callback(c_msg.as_ptr(), user_data_ptr as *mut c_void);
    });

    MoqMgrResult::Success
}

/// Add a subscription to the session (for consumer mode)
/// Must be called before moq_mgr_session_start
#[no_mangle]
pub extern "C" fn moq_mgr_session_add_subscription(
    session: *mut MoqMgrSession,
    track_name: *const c_char,
    callback: MoqMgrDataCallback,
    user_data: *mut c_void,
) -> MoqMgrResult {
    if session.is_null() || track_name.is_null() {
        return MoqMgrResult::ErrorInvalidParameter;
    }

    let session = unsafe { &*session };
    
    let track_name_str = unsafe {
        match CStr::from_ptr(track_name).to_str() {
            Ok(s) => s.to_string(),
            Err(_) => return MoqMgrResult::ErrorInvalidParameter,
        }
    };

    let user_data_ptr = user_data as usize;
    
    let data_callback = Arc::new(move |data: &[u8]| {
        callback(data.as_ptr(), data.len(), user_data_ptr as *mut c_void);
    });

    let subscription = SubscriptionConfig {
        moq_track_name: track_name_str,
        data_callback,
    };

    session.session.add_subscription(subscription);
    MoqMgrResult::Success
}

/// Add a broadcast to the session (for producer mode)
/// Must be called before moq_mgr_session_start
#[no_mangle]
pub extern "C" fn moq_mgr_session_add_broadcast(
    session: *mut MoqMgrSession,
    track_name: *const c_char,
    priority: u32,
) -> MoqMgrResult {
    if session.is_null() || track_name.is_null() {
        return MoqMgrResult::ErrorInvalidParameter;
    }

    let session = unsafe { &*session };
    
    let track_name_str = unsafe {
        match CStr::from_ptr(track_name).to_str() {
            Ok(s) => s.to_string(),
            Err(_) => return MoqMgrResult::ErrorInvalidParameter,
        }
    };

    let broadcast = BroadcastConfig {
        moq_track_name: track_name_str,
        priority: priority.try_into().unwrap_or(50),
    };

    session.session.add_broadcast(broadcast);
    MoqMgrResult::Success
}

/// Start the session and connect to the MoQ server
#[no_mangle]
pub extern "C" fn moq_mgr_session_start(session: *mut MoqMgrSession) -> MoqMgrResult {
    if session.is_null() {
        return MoqMgrResult::ErrorInvalidParameter;
    }

    let session = unsafe { &*session };
    let session_arc = session.session.clone();
    let runtime = session.runtime.clone();
    
    // Spawn the start operation in the background
    runtime.spawn(async move {
        // Call start on the session
        if let Err(e) = session_arc.start().await {
            tracing::error!("Failed to start session: {}", e);
        }
    });
    
    MoqMgrResult::Success
}

/// Stop the session and disconnect from the MoQ server
#[no_mangle]
pub extern "C" fn moq_mgr_session_stop(session: *mut MoqMgrSession) -> MoqMgrResult {
    if session.is_null() {
        return MoqMgrResult::ErrorInvalidParameter;
    }

    let session = unsafe { &*session };
    session.session.stop();
    MoqMgrResult::Success
}

/// Check if the session is running
#[no_mangle]
pub extern "C" fn moq_mgr_session_is_running(session: *mut MoqMgrSession) -> i32 {
    if session.is_null() {
        return 0;
    }

    let session = unsafe { &*session };
    if session.session.is_running() {
        1
    } else {
        0
    }
}

/// Destroy the session and free all resources
#[no_mangle]
pub extern "C" fn moq_mgr_session_destroy(session: *mut MoqMgrSession) {
    if !session.is_null() {
        unsafe {
            let session = Box::from_raw(session);
            session.session.stop();
            drop(session);
        }
    }
}

/// Get the last error message (thread-local)
#[no_mangle]
pub extern "C" fn moq_mgr_get_last_error() -> *const c_char {
    // TODO: Implement thread-local error storage
    std::ptr::null()
}
