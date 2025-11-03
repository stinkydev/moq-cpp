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

/// Log callback function type
/// Parameters: level (0=ERROR, 1=WARN, 2=INFO, 3=DEBUG, 4=TRACE), message, user_data
pub type MoqMgrLogCallback = extern "C" fn(i32, *const c_char, *mut c_void);

// Global storage for the log callback
static mut LOG_CALLBACK: Option<(MoqMgrLogCallback, *mut c_void)> = None;
static LOG_CALLBACK_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Initialize the MoQ Manager library
/// This should be called once at startup
#[no_mangle]
pub extern "C" fn moq_mgr_init() -> MoqMgrResult {
    // Initialize basic tracing to stdout as fallback
    let _ = tracing_subscriber::fmt::try_init();
    MoqMgrResult::Success
}

/// Initialize the MoQ Manager library with custom log callback
/// This should be called once at startup if you want to receive log messages
/// 
/// Parameters:
/// - log_callback: Function to receive log messages
/// - user_data: User data pointer passed to log callback
/// - include_moq_libs: If true, include logs from moq-lite/moq-native; if false, only moq-mgr logs
#[no_mangle]
pub extern "C" fn moq_mgr_init_with_logging(
    log_callback: MoqMgrLogCallback,
    user_data: *mut c_void,
    include_moq_libs: i32,
) -> MoqMgrResult {
    let _lock = LOG_CALLBACK_LOCK.lock().unwrap();
    
    // Store the callback globally
    unsafe {
        LOG_CALLBACK = Some((log_callback, user_data));
    }
    
    // Initialize tracing with our custom subscriber
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
    
    let callback_layer = CallbackLayer::new();
    
    // Create filter based on include_moq_libs flag
    let filter = if include_moq_libs != 0 {
        // Include all logs
        EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("debug"))
    } else {
        // Only include moq_mgr logs
        EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("moq_mgr=debug"))
    };
    
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(callback_layer)
        .try_init();
    
    MoqMgrResult::Success
}

/// Create a new MoQ Manager session
/// 
/// Parameters:
/// - server_url: The MoQ server URL (e.g., "https://relay.moq.example.com:4433")
/// - namespace: The broadcast namespace to use
/// - mode: Session mode (0=PublishOnly, 1=SubscribeOnly)
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
/// - mode: Session mode (0=PublishOnly, 1=SubscribeOnly)
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
        reconnect_callback: None, // FFI layer doesn't provide reconnect callbacks - session handles it
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

// Custom tracing layer that forwards logs to the C callback
struct CallbackLayer;

impl CallbackLayer {
    fn new() -> Self {
        Self
    }
}

impl<S> tracing_subscriber::Layer<S> for CallbackLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let _lock = LOG_CALLBACK_LOCK.lock().unwrap();
        
        unsafe {
            if let Some((callback, user_data)) = LOG_CALLBACK {
                // Convert tracing level to our integer representation
                let level = match *event.metadata().level() {
                    tracing::Level::ERROR => 0,
                    tracing::Level::WARN => 1,
                    tracing::Level::INFO => 2,
                    tracing::Level::DEBUG => 3,
                    tracing::Level::TRACE => 4,
                };
                
                // Format the message
                let mut visitor = MessageVisitor::new();
                event.record(&mut visitor);
                
                // Create a C string for the message
                if let Ok(c_message) = CString::new(visitor.message) {
                    callback(level, c_message.as_ptr(), user_data);
                }
            }
        }
    }
}

// Visitor to extract the message from tracing events
struct MessageVisitor {
    message: String,
}

impl MessageVisitor {
    fn new() -> Self {
        Self {
            message: String::new(),
        }
    }
}

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{:?}", value);
            // Remove quotes from debug formatted strings
            if self.message.starts_with('"') && self.message.ends_with('"') {
                self.message = self.message[1..self.message.len()-1].to_string();
            }
        } else {
            if !self.message.is_empty() {
                self.message.push(' ');
            }
            self.message.push_str(&format!("{}={:?}", field.name(), value));
        }
    }
    
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            if !self.message.is_empty() {
                self.message.push(' ');
            }
            self.message.push_str(&format!("{}={}", field.name(), value));
        }
    }
}
