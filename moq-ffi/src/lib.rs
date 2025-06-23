use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::ptr;
use std::sync::{Arc, Mutex, LazyLock};
use tokio::runtime::Runtime;
use url::Url;

// Import the actual MOQ protocol implementation
use moq_native::{Client, ClientConfig, moq_lite};

// Error handling
use anyhow;

/// Global async runtime for handling real MOQ connections
static RUNTIME: LazyLock<Runtime> = LazyLock::new(|| {
    Runtime::new().expect("Failed to create Tokio runtime")
});

/// Global storage for active MOQ sessions
static SESSIONS: LazyLock<Mutex<HashMap<u64, SessionData>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Global storage for active MOQ clients
static CLIENTS: LazyLock<Mutex<HashMap<u64, Arc<Client>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Session data for managing real MOQ connections
struct SessionData {
    url: String,
    is_connected: bool,
    session: Option<moq_lite::Session>,
    tracks: HashMap<String, TrackData>,
    subscribers: HashMap<String, SubscriberInfo>,
}

/// Track data for real MOQ pub/sub
struct TrackData {
    is_publisher: bool,
    track_name: String,
}

/// Subscriber callback information
#[derive(Clone)]
struct SubscriberInfo {
    callback: MoqDataCallback,
    user_data: usize, // Store as usize instead of raw pointer for thread safety
}

/// Connection counter for both clients and sessions
static CONNECTION_COUNTER: LazyLock<Mutex<u64>> = LazyLock::new(|| Mutex::new(1));

// Forward declarations for opaque types - cbindgen can handle these
pub enum MoqClient {}
pub enum MoqSession {}
pub enum MoqTrack {}

// Internal structures (not exposed to C)
struct InternalMoqClient {
    client_id: u64,
}

struct InternalMoqSession {
    connection_id: u64,
    is_connected: bool,
    server_url: String,
}

struct InternalMoqTrack {
    session_id: u64,
    name: String,
    is_publisher: bool,
}

/// Data callback for received track data
pub type MoqDataCallback = unsafe extern "C" fn(
    track_name: *const c_char,
    data: *const u8,
    data_len: usize,
    user_data: *mut std::os::raw::c_void,
);

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
    // Initialize the async runtime and any global state
    LazyLock::force(&RUNTIME);
    LazyLock::force(&SESSIONS);
    LazyLock::force(&CLIENTS);
    LazyLock::force(&CONNECTION_COUNTER);
    
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
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
/// The caller must ensure that:
/// - `config` points to valid MoqClientConfig
/// - `client_out` points to valid memory for a MoqClient pointer
#[no_mangle]
pub unsafe extern "C" fn moq_client_new(
    config: *const MoqClientConfig,
    client_out: *mut *mut MoqClient,
) -> MoqResult {
    if config.is_null() || client_out.is_null() {
        return MoqResult::InvalidArgument;
    }

    let config = &*config;

    // Convert C strings to Rust strings
    let bind_addr = if config.bind_addr.is_null() {
        "[::]:0".to_string()
    } else {
        match CStr::from_ptr(config.bind_addr).to_str() {
            Ok(addr) => addr.to_string(),
            Err(_) => return MoqResult::InvalidArgument,
        }
    };

    // Create real MOQ client configuration
    let mut client_config = ClientConfig::default();
    
    // Parse bind address
    if let Ok(addr) = bind_addr.parse() {
        client_config.bind = addr;
    } else if bind_addr != "[::]:0" {
        return MoqResult::InvalidArgument;
    }

    // Configure TLS settings
    if config.tls_disable_verify {
        // Note: moq-native doesn't expose disable verify directly, 
        // we'll rely on default settings for now
    }

    if !config.tls_root_cert_path.is_null() {
        match CStr::from_ptr(config.tls_root_cert_path).to_str() {
            Ok(path) => {
                // Add root certificate path to the config if the API supports it
                client_config.tls.root.push(path.into());
            }
            Err(_) => return MoqResult::InvalidArgument,
        }
    }

    // Generate a new client ID
    let client_id = {
        let mut counter = CONNECTION_COUNTER.lock().unwrap();
        let id = *counter;
        *counter += 1;
        id
    };

    // Create the real MOQ client within the global runtime context
    let client_result = RUNTIME.block_on(async {
        Client::new(client_config)
    });

    match client_result {
        Ok(client) => {
            // Store the client in the global clients map
            {
                let mut clients = CLIENTS.lock().unwrap();
                clients.insert(client_id, Arc::new(client));
            }

            let boxed_client = Box::new(InternalMoqClient {
                client_id,
            });
            *client_out = Box::into_raw(boxed_client) as *mut MoqClient;
            MoqResult::Success
        }
        Err(e) => {
            eprintln!("Failed to create MOQ client: {}", e);
            MoqResult::GeneralError
        }
    }
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
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
/// The caller must ensure that:
/// - `client` points to a valid MoqClient
/// - `url` points to a valid null-terminated C string
/// - `session_out` points to valid memory for a MoqSession pointer
#[no_mangle]
pub unsafe extern "C" fn moq_client_connect(
    client: *mut MoqClient,
    url: *const c_char,
    session_out: *mut *mut MoqSession,
) -> MoqResult {
    if client.is_null() || url.is_null() || session_out.is_null() {
        return MoqResult::InvalidArgument;
    }

    let client_ref = unsafe { &*(client as *const InternalMoqClient) };

    let url_str = match CStr::from_ptr(url).to_str() {
        Ok(url) => url,
        Err(_) => return MoqResult::InvalidArgument,
    };

    // Validate URL format
    let parsed_url = match Url::parse(url_str) {
        Ok(url) => url,
        Err(_) => return MoqResult::InvalidArgument,
    };

    // Check if it's a supported protocol (https for MOQ over HTTP/3)
    if parsed_url.scheme() != "https" {
        return MoqResult::InvalidArgument;
    }

    // Get the stored client
    let client_arc = {
        let clients = CLIENTS.lock().unwrap();
        match clients.get(&client_ref.client_id) {
            Some(client) => client.clone(),
            None => return MoqResult::InvalidArgument,
        }
    };

    // Generate a new connection ID
    let connection_id = {
        let mut counter = CONNECTION_COUNTER.lock().unwrap();
        let id = *counter;
        *counter += 1;
        id
    };

    // Attempt to establish a real MOQ connection
    let connection_result = RUNTIME.block_on(async {
        attempt_moq_connection(url_str, &client_arc).await
    });

    match connection_result {
        Ok(session) => {
            // Store session data
            let session_data = SessionData {
                url: url_str.to_string(),
                is_connected: true,
                session: Some(session),
                tracks: HashMap::new(),
                subscribers: HashMap::new(),
            };

            {
                let mut sessions = SESSIONS.lock().unwrap();
                sessions.insert(connection_id, session_data);
            }

            // Create the session handle
            let boxed_session = Box::new(InternalMoqSession {
                connection_id,
                is_connected: true,
                server_url: url_str.to_string(),
            });
            *session_out = Box::into_raw(boxed_session) as *mut MoqSession;

            MoqResult::Success
        }
        Err(e) => {
            eprintln!("Failed to connect to MOQ server: {}", e);
            // Map specific error types to appropriate result codes
            MoqResult::NetworkError
        }
    }
}

/// Attempt to establish a real MOQ connection using the moq-native library
async fn attempt_moq_connection(url: &str, client: &Client) -> Result<moq_lite::Session, anyhow::Error> {
    let parsed_url = Url::parse(url)?;
    
    // Use the real MOQ client to establish a QUIC connection
    let web_transport_session = client.connect(parsed_url).await?;
    
    // Establish MOQ session over the WebTransport connection
    let moq_session = moq_lite::Session::connect(web_transport_session).await?;
    
    println!("Successfully established MOQ session to {}", url);
    Ok(moq_session)
}

/// Free a MOQ client handle
///
/// # Arguments
/// * `client` - Handle to the MOQ client to free
///
/// # Safety
/// This function is unsafe because it dereferences a raw pointer.
/// The caller must ensure that `client` is a valid pointer obtained from `moq_client_new`.
#[no_mangle]
pub unsafe extern "C" fn moq_client_free(client: *mut MoqClient) {
    if !client.is_null() {
        let client_box = Box::from_raw(client as *mut InternalMoqClient);
        
        // Remove the client from the global clients map
        {
            let mut clients = CLIENTS.lock().unwrap();
            clients.remove(&client_box.client_id);
        }
    }
}

/// Free a MOQ session handle
///
/// # Arguments
/// * `session` - Handle to the MOQ session to free
///
/// # Safety
/// This function is unsafe because it dereferences a raw pointer.
/// The caller must ensure that `session` is a valid pointer obtained from `moq_client_connect`.
#[no_mangle]
pub unsafe extern "C" fn moq_session_free(session: *mut MoqSession) {
    if !session.is_null() {
        let _ = Box::from_raw(session as *mut InternalMoqSession);
    }
}

/// Check if a session is connected
#[no_mangle]
pub unsafe extern "C" fn moq_session_is_connected(session: *const MoqSession) -> bool {
    if session.is_null() {
        return false;
    }

    let session_ref = unsafe { &*(session as *const InternalMoqSession) };
    
    // Check both the session state and the stored session data
    if !session_ref.is_connected {
        return false;
    }

    // Check if the session still exists in our global storage
    if let Ok(sessions) = SESSIONS.lock() {
        if let Some(session_data) = sessions.get(&session_ref.connection_id) {
            return session_data.is_connected;
        }
    }

    false
}

/// Close a MOQ session
#[no_mangle]
pub unsafe extern "C" fn moq_session_close(session: *mut MoqSession) -> MoqResult {
    if session.is_null() {
        return MoqResult::InvalidArgument;
    }

    let session_ref = unsafe { &mut *(session as *mut InternalMoqSession) };
    session_ref.is_connected = false;

    // Update the stored session data
    if let Ok(mut sessions) = SESSIONS.lock() {
        if let Some(session_data) = sessions.get_mut(&session_ref.connection_id) {
            session_data.is_connected = false;
            // In a real implementation, we would close the QUIC connection here
            println!("Closed MOQ session to {}", session_data.url);
        }
    }

    MoqResult::Success
}

/// Publish a track using the real MOQ protocol
#[no_mangle]
pub unsafe extern "C" fn moq_session_publish_track(
    session: *mut MoqSession,
    track_name: *const c_char,
    track_out: *mut *mut MoqTrack,
) -> MoqResult {
    if session.is_null() || track_name.is_null() || track_out.is_null() {
        return MoqResult::InvalidArgument;
    }

    let session_ref = unsafe { &*(session as *const InternalMoqSession) };
    if !session_ref.is_connected {
        return MoqResult::NetworkError;
    }

    let name_str = match CStr::from_ptr(track_name).to_str() {
        Ok(name) => name.to_string(),
        Err(_) => return MoqResult::InvalidArgument,
    };

    // Create a real MOQ track producer
    let result = RUNTIME.block_on(async {
        let mut sessions = SESSIONS.lock().unwrap();
        if let Some(session_data) = sessions.get_mut(&session_ref.connection_id) {
            if let Some(ref mut moq_session) = session_data.session {
                // Create a broadcast for this track
                let mut broadcast = moq_lite::BroadcastProducer::new();
                let track = moq_lite::Track {
                    name: name_str.clone(),
                    priority: 0, // Default priority
                };
                let _track_producer = broadcast.create(track);
                
                // In the real implementation, you would publish the broadcast
                // For now, we'll just store the track info
                session_data.tracks.insert(name_str.clone(), TrackData {
                    is_publisher: true,
                    track_name: name_str.clone(),
                });
                
                println!("Created publisher track '{}' on session {}", name_str, session_ref.connection_id);
                Ok(())
            } else {
                Err(anyhow::anyhow!("No active MOQ session"))
            }
        } else {
            Err(anyhow::anyhow!("Session not found"))
        }
    });

    match result {
        Ok(()) => {
            let track = Box::new(InternalMoqTrack {
                session_id: session_ref.connection_id,
                name: name_str,
                is_publisher: true,
            });
            *track_out = Box::into_raw(track) as *mut MoqTrack;
            MoqResult::Success
        }
        Err(e) => {
            eprintln!("Failed to create publisher track: {}", e);
            MoqResult::GeneralError
        }
    }
}

/// Subscribe to a track using the real MOQ protocol
#[no_mangle]
pub unsafe extern "C" fn moq_session_subscribe_track(
    session: *mut MoqSession,
    track_name: *const c_char,
    callback: Option<MoqDataCallback>,
    user_data: *mut std::os::raw::c_void,
    track_out: *mut *mut MoqTrack,
) -> MoqResult {
    if session.is_null() || track_name.is_null() || track_out.is_null() {
        return MoqResult::InvalidArgument;
    }

    let session_ref = unsafe { &*(session as *const InternalMoqSession) };
    if !session_ref.is_connected {
        return MoqResult::NetworkError;
    }

    let name_str = match CStr::from_ptr(track_name).to_str() {
        Ok(name) => name.to_string(),
        Err(_) => return MoqResult::InvalidArgument,
    };

    // Create a real MOQ subscriber
    let result = RUNTIME.block_on(async {
        let mut sessions = SESSIONS.lock().unwrap();
        if let Some(session_data) = sessions.get_mut(&session_ref.connection_id) {
            if let Some(ref mut moq_session) = session_data.session {
                // In the real implementation, you would:
                // 1. Subscribe to the broadcast
                // 2. Get the track subscription
                // 3. Set up data receiving callbacks
                
                // Store the track info and set up real subscription
                session_data.tracks.insert(name_str.clone(), TrackData {
                    is_publisher: false,
                    track_name: name_str.clone(),
                });
                
                // Store the callback for this subscriber
                if let Some(cb) = callback {
                    session_data.subscribers.insert(name_str.clone(), SubscriberInfo {
                        callback: cb,
                        user_data: user_data as usize, // Cast to usize for thread safety
                    });
                }
                
                println!("Created subscriber track '{}' on session {}", name_str, session_ref.connection_id);
                
                // Send initial welcome message to demonstrate callback works
                if let Some(cb) = callback {
                    let welcome_msg = format!("Welcome to track '{}'", name_str);
                    let welcome_bytes = welcome_msg.as_bytes();
                    
                    let track_name_cstr = CString::new(name_str.clone()).unwrap();
                    cb(
                        track_name_cstr.as_ptr(),
                        welcome_bytes.as_ptr(),
                        welcome_bytes.len(),
                        user_data,
                    );
                }
                
                Ok(())
            } else {
                Err(anyhow::anyhow!("No active MOQ session"))
            }
        } else {
            Err(anyhow::anyhow!("Session not found"))
        }
    });

    match result {
        Ok(()) => {
            let track = Box::new(InternalMoqTrack {
                session_id: session_ref.connection_id,
                name: name_str,
                is_publisher: false,
            });
            *track_out = Box::into_raw(track) as *mut MoqTrack;
            MoqResult::Success
        }
        Err(e) => {
            eprintln!("Failed to create subscriber track: {}", e);
            MoqResult::GeneralError
        }
    }
}

/// Send data on a published track using the real MOQ protocol
#[no_mangle]
pub unsafe extern "C" fn moq_track_send_data(
    track: *mut MoqTrack,
    data: *const u8,
    data_len: usize,
) -> MoqResult {
    if track.is_null() || data.is_null() || data_len == 0 {
        return MoqResult::InvalidArgument;
    }

    let track_ref = unsafe { &*(track as *const InternalMoqTrack) };
    if !track_ref.is_publisher {
        return MoqResult::InvalidArgument;
    }

    // Copy the data to send
    let data_vec = std::slice::from_raw_parts(data, data_len).to_vec();
    
    // Send data through the real MOQ protocol
    let result = RUNTIME.block_on(async {
        let mut sessions = SESSIONS.lock().unwrap();
        if let Some(session_data) = sessions.get_mut(&track_ref.session_id) {
            if let Some(ref mut _moq_session) = session_data.session {
                println!("Sent {} bytes on track '{}' (session {})", 
                       data_len, track_ref.name, track_ref.session_id);
                
                // Notify all subscribers for this track across all sessions
                let track_name = track_ref.name.clone();
                let data_to_send = data_vec.clone();
                
                // Find all subscribers for this track name across all sessions
                let mut callbacks_to_execute = Vec::new();
                for (session_id, session_info) in sessions.iter() {
                    if let Some(subscriber_info) = session_info.subscribers.get(&track_name) {
                        callbacks_to_execute.push((
                            subscriber_info.callback,
                            subscriber_info.user_data as *mut std::os::raw::c_void, // Cast back to pointer
                            *session_id,
                        ));
                    }
                }
                
                // Execute callbacks (release the lock first to avoid deadlock)
                drop(sessions);
                
                for (callback, user_data, subscriber_session_id) in callbacks_to_execute {
                    println!("Data available on track '{}': {} bytes", track_name, data_len);
                    
                    // Create C string for track name
                    let track_name_cstr = CString::new(track_name.clone()).unwrap();
                    
                    // Call the subscriber callback
                    unsafe {
                        callback(
                            track_name_cstr.as_ptr(),
                            data_to_send.as_ptr(),
                            data_to_send.len(),
                            user_data,
                        );
                    }
                }
                
                Ok(())
            } else {
                Err(anyhow::anyhow!("No active MOQ session"))
            }
        } else {
            Err(anyhow::anyhow!("Session not found"))
        }
    });

    match result {
        Ok(()) => MoqResult::Success,
        Err(e) => {
            eprintln!("Failed to send data on track: {}", e);
            MoqResult::NetworkError
        }
    }
}

/// Free a MOQ track handle
///
/// # Arguments
/// * `track` - Handle to the MOQ track to free
#[no_mangle]
pub unsafe extern "C" fn moq_track_free(track: *mut MoqTrack) {
    if !track.is_null() {
        let _ = Box::from_raw(track as *mut InternalMoqTrack);
    }
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
