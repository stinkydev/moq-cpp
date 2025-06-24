use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::ptr;
use std::sync::{Arc, Mutex, LazyLock};
use tokio::runtime::Runtime;

/// Global async runtime for handling MOQ operations
static RUNTIME: LazyLock<Runtime> = LazyLock::new(|| {
    Runtime::new().expect("Failed to create Tokio runtime")
});

/// Global storage for all MOQ handles
static HANDLES: LazyLock<Mutex<HandleStorage>> = LazyLock::new(|| Mutex::new(HandleStorage::new()));

/// ID counter for all handles
static ID_COUNTER: LazyLock<Mutex<u64>> = LazyLock::new(|| Mutex::new(1));

/// Storage for all MOQ handles
struct HandleStorage {
    clients: HashMap<u64, ClientData>,
    sessions: HashMap<u64, SessionData>,
    broadcast_producers: HashMap<u64, BroadcastProducerData>,
    broadcast_consumers: HashMap<u64, BroadcastConsumerData>,
    track_producers: HashMap<u64, TrackProducerData>,
    track_consumers: HashMap<u64, TrackConsumerData>,
    group_producers: HashMap<u64, GroupProducerData>,
    group_consumers: HashMap<u64, GroupConsumerData>,
}

impl HandleStorage {
    fn new() -> Self {
        Self {
            clients: HashMap::new(),
            sessions: HashMap::new(),
            broadcast_producers: HashMap::new(),
            broadcast_consumers: HashMap::new(),
            track_producers: HashMap::new(),
            track_consumers: HashMap::new(),
            group_producers: HashMap::new(),
            group_consumers: HashMap::new(),
        }
    }
}

/// Client data
struct ClientData {
    config: ClientConfig,
}

/// Session data
struct SessionData {
    client_id: u64,
    url: String,
    is_connected: bool,
}

/// Broadcast producer data
struct BroadcastProducerData {
    name: String,
    tracks: Vec<u64>, // Track producer IDs
}

/// Broadcast consumer data
struct BroadcastConsumerData {
    session_id: u64,
    name: String,
    tracks: Vec<u64>, // Track consumer IDs
}

/// Track producer data
struct TrackProducerData {
    broadcast_id: u64,
    name: String,
    priority: u32,
    groups: Vec<u64>, // Group producer IDs
}

/// Track consumer data
struct TrackConsumerData {
    broadcast_id: u64,
    name: String,
    priority: u32,
    groups: Vec<u64>, // Group consumer IDs
}

/// Group producer data
struct GroupProducerData {
    track_id: u64,
    sequence: u64,
    frames: Vec<Vec<u8>>,
    finished: bool,
}

/// Group consumer data
struct GroupConsumerData {
    track_id: u64,
    sequence: u64,
    frames: Vec<Vec<u8>>,
    current_frame: usize,
}

/// Client configuration
#[derive(Clone)]
struct ClientConfig {
    bind_addr: String,
    tls_disable_verify: bool,
    tls_root_cert_path: String,
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

/// Opaque handle for the MOQ client
#[repr(C)]
pub struct MoqClient {
    pub id: u64,
}

/// Opaque handle for a MOQ session
#[repr(C)]
pub struct MoqSession {
    pub id: u64,
}

/// Opaque handle for a MOQ broadcast producer
#[repr(C)]
pub struct MoqBroadcastProducer {
    pub id: u64,
}

/// Opaque handle for a MOQ broadcast consumer
#[repr(C)]
pub struct MoqBroadcastConsumer {
    pub id: u64,
}

/// Opaque handle for a MOQ track producer
#[repr(C)]
pub struct MoqTrackProducer {
    pub id: u64,
}

/// Opaque handle for a MOQ track consumer
#[repr(C)]
pub struct MoqTrackConsumer {
    pub id: u64,
}

/// Opaque handle for a MOQ group producer
#[repr(C)]
pub struct MoqGroupProducer {
    pub id: u64,
}

/// Opaque handle for a MOQ group consumer
#[repr(C)]
pub struct MoqGroupConsumer {
    pub id: u64,
}

/// Track information
#[repr(C)]
pub struct MoqTrack {
    pub name: *const c_char,
    pub priority: u32,
}

/// Generate a new unique ID
fn next_id() -> u64 {
    let mut counter = ID_COUNTER.lock().unwrap();
    let id = *counter;
    *counter += 1;
    id
}

/// Initialize the MOQ FFI library
#[no_mangle]
pub extern "C" fn moq_init() -> MoqResult {
    // Initialize static resources
    LazyLock::force(&RUNTIME);
    LazyLock::force(&HANDLES);
    LazyLock::force(&ID_COUNTER);
    
    MoqResult::Success
}

/// Create a new MOQ client
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

    let tls_root_cert_path = if config.tls_root_cert_path.is_null() {
        String::new()
    } else {
        match CStr::from_ptr(config.tls_root_cert_path).to_str() {
            Ok(path) => path.to_string(),
            Err(_) => return MoqResult::InvalidArgument,
        }
    };

    let client_config = ClientConfig {
        bind_addr,
        tls_disable_verify: config.tls_disable_verify,
        tls_root_cert_path,
    };

    let client_id = next_id();
    let client_data = ClientData {
        config: client_config,
    };

    // Store the client data
    {
        let mut handles = HANDLES.lock().unwrap();
        handles.clients.insert(client_id, client_data);
    }

    // Create and return the client handle
    let boxed_client = Box::new(MoqClient { id: client_id });
    *client_out = Box::into_raw(boxed_client);

    MoqResult::Success
}

/// Connect to a MOQ server
#[no_mangle]
pub unsafe extern "C" fn moq_client_connect(
    client: *mut MoqClient,
    url: *const c_char,
    session_out: *mut *mut MoqSession,
) -> MoqResult {
    if client.is_null() || url.is_null() || session_out.is_null() {
        return MoqResult::InvalidArgument;
    }

    let client = &*client;
    let url_str = match CStr::from_ptr(url).to_str() {
        Ok(url) => url,
        Err(_) => return MoqResult::InvalidArgument,
    };

    // Validate that the client exists
    {
        let handles = HANDLES.lock().unwrap();
        if !handles.clients.contains_key(&client.id) {
            return MoqResult::InvalidArgument;
        }
    }

    let session_id = next_id();
    let session_data = SessionData {
        client_id: client.id,
        url: url_str.to_string(),
        is_connected: true, // For now, assume connection always succeeds
    };

    // Store the session data
    {
        let mut handles = HANDLES.lock().unwrap();
        handles.sessions.insert(session_id, session_data);
    }

    // Create and return the session handle
    let boxed_session = Box::new(MoqSession { id: session_id });
    *session_out = Box::into_raw(boxed_session);

    MoqResult::Success
}

/// Free a MOQ client handle
#[no_mangle]
pub unsafe extern "C" fn moq_client_free(client: *mut MoqClient) {
    if !client.is_null() {
        let client = Box::from_raw(client);
        
        // Remove from storage
        let mut handles = HANDLES.lock().unwrap();
        handles.clients.remove(&client.id);
    }
}

/// Free a MOQ session handle
#[no_mangle]
pub unsafe extern "C" fn moq_session_free(session: *mut MoqSession) {
    if !session.is_null() {
        let session = Box::from_raw(session);
        
        // Remove from storage
        let mut handles = HANDLES.lock().unwrap();
        handles.sessions.remove(&session.id);
    }
}

/// Check if a session is connected
#[no_mangle]
pub unsafe extern "C" fn moq_session_is_connected(session: *const MoqSession) -> bool {
    if session.is_null() {
        return false;
    }

    let session = &*session;
    let handles = HANDLES.lock().unwrap();
    
    if let Some(session_data) = handles.sessions.get(&session.id) {
        session_data.is_connected
    } else {
        false
    }
}

/// Close a MOQ session
#[no_mangle]
pub unsafe extern "C" fn moq_session_close(session: *mut MoqSession) -> MoqResult {
    if session.is_null() {
        return MoqResult::InvalidArgument;
    }

    let session = &*session;
    let mut handles = HANDLES.lock().unwrap();
    
    if let Some(session_data) = handles.sessions.get_mut(&session.id) {
        session_data.is_connected = false;
        MoqResult::Success
    } else {
        MoqResult::InvalidArgument
    }
}

/// Create a new broadcast producer
#[no_mangle]
pub extern "C" fn moq_broadcast_producer_new(
    producer_out: *mut *mut MoqBroadcastProducer,
) -> MoqResult {
    if producer_out.is_null() {
        return MoqResult::InvalidArgument;
    }

    let producer_id = next_id();
    let producer_data = BroadcastProducerData {
        name: String::new(),
        tracks: Vec::new(),
    };

    // Store the producer data
    {
        let mut handles = HANDLES.lock().unwrap();
        handles.broadcast_producers.insert(producer_id, producer_data);
    }

    // Create and return the producer handle
    let boxed_producer = Box::new(MoqBroadcastProducer { id: producer_id });
    unsafe {
        *producer_out = Box::into_raw(boxed_producer);
    }

    MoqResult::Success
}

/// Create a track producer within a broadcast
#[no_mangle]
pub unsafe extern "C" fn moq_broadcast_producer_create_track(
    producer: *mut MoqBroadcastProducer,
    track: *const MoqTrack,
    track_out: *mut *mut MoqTrackProducer,
) -> MoqResult {
    if producer.is_null() || track.is_null() || track_out.is_null() {
        return MoqResult::InvalidArgument;
    }

    let producer = &*producer;
    let track_info = &*track;

    let track_name = if track_info.name.is_null() {
        String::new()
    } else {
        match CStr::from_ptr(track_info.name).to_str() {
            Ok(name) => name.to_string(),
            Err(_) => return MoqResult::InvalidArgument,
        }
    };

    let track_id = next_id();
    let track_data = TrackProducerData {
        broadcast_id: producer.id,
        name: track_name,
        priority: track_info.priority,
        groups: Vec::new(),
    };

    // Store the track data and update the broadcast
    {
        let mut handles = HANDLES.lock().unwrap();
        handles.track_producers.insert(track_id, track_data);
        
        if let Some(broadcast) = handles.broadcast_producers.get_mut(&producer.id) {
            broadcast.tracks.push(track_id);
        } else {
            return MoqResult::InvalidArgument;
        }
    }

    // Create and return the track handle
    let boxed_track = Box::new(MoqTrackProducer { id: track_id });
    *track_out = Box::into_raw(boxed_track);

    MoqResult::Success
}

/// Publish a broadcast on a session
#[no_mangle]
pub unsafe extern "C" fn moq_session_publish(
    session: *mut MoqSession,
    broadcast_name: *const c_char,
    producer: *mut MoqBroadcastProducer,
) -> MoqResult {
    if session.is_null() || broadcast_name.is_null() || producer.is_null() {
        return MoqResult::InvalidArgument;
    }

    let session = &*session;
    let producer = &*producer;

    let name = match CStr::from_ptr(broadcast_name).to_str() {
        Ok(name) => name.to_string(),
        Err(_) => return MoqResult::InvalidArgument,
    };

    // Update the broadcast producer with the name
    {
        let mut handles = HANDLES.lock().unwrap();
        
        // Validate session exists
        if !handles.sessions.contains_key(&session.id) {
            return MoqResult::InvalidArgument;
        }
        
        // Update broadcast name
        if let Some(broadcast) = handles.broadcast_producers.get_mut(&producer.id) {
            broadcast.name = name;
        } else {
            return MoqResult::InvalidArgument;
        }
    }

    MoqResult::Success
}

/// Consume a broadcast from a session
#[no_mangle]
pub unsafe extern "C" fn moq_session_consume(
    session: *mut MoqSession,
    broadcast_name: *const c_char,
    consumer_out: *mut *mut MoqBroadcastConsumer,
) -> MoqResult {
    if session.is_null() || broadcast_name.is_null() || consumer_out.is_null() {
        return MoqResult::InvalidArgument;
    }

    let session = &*session;

    let name = match CStr::from_ptr(broadcast_name).to_str() {
        Ok(name) => name.to_string(),
        Err(_) => return MoqResult::InvalidArgument,
    };

    // Validate session exists
    {
        let handles = HANDLES.lock().unwrap();
        if !handles.sessions.contains_key(&session.id) {
            return MoqResult::InvalidArgument;
        }
    }

    let consumer_id = next_id();
    let consumer_data = BroadcastConsumerData {
        session_id: session.id,
        name,
        tracks: Vec::new(),
    };

    // Store the consumer data
    {
        let mut handles = HANDLES.lock().unwrap();
        handles.broadcast_consumers.insert(consumer_id, consumer_data);
    }

    // Create and return the consumer handle
    let boxed_consumer = Box::new(MoqBroadcastConsumer { id: consumer_id });
    *consumer_out = Box::into_raw(boxed_consumer);

    MoqResult::Success
}

/// Subscribe to a track within a broadcast
#[no_mangle]
pub unsafe extern "C" fn moq_broadcast_consumer_subscribe_track(
    consumer: *mut MoqBroadcastConsumer,
    track: *const MoqTrack,
    track_out: *mut *mut MoqTrackConsumer,
) -> MoqResult {
    if consumer.is_null() || track.is_null() || track_out.is_null() {
        return MoqResult::InvalidArgument;
    }

    let consumer = &*consumer;
    let track_info = &*track;

    let track_name = if track_info.name.is_null() {
        String::new()
    } else {
        match CStr::from_ptr(track_info.name).to_str() {
            Ok(name) => name.to_string(),
            Err(_) => return MoqResult::InvalidArgument,
        }
    };

    let track_id = next_id();
    let track_data = TrackConsumerData {
        broadcast_id: consumer.id,
        name: track_name,
        priority: track_info.priority,
        groups: Vec::new(),
    };

    // Store the track data and update the broadcast
    {
        let mut handles = HANDLES.lock().unwrap();
        handles.track_consumers.insert(track_id, track_data);
        
        if let Some(broadcast) = handles.broadcast_consumers.get_mut(&consumer.id) {
            broadcast.tracks.push(track_id);
        } else {
            return MoqResult::InvalidArgument;
        }
    }

    // Create and return the track handle
    let boxed_track = Box::new(MoqTrackConsumer { id: track_id });
    *track_out = Box::into_raw(boxed_track);

    MoqResult::Success
}

/// Create a group within a track producer
#[no_mangle]
pub unsafe extern "C" fn moq_track_producer_create_group(
    track: *mut MoqTrackProducer,
    sequence: u64,
    group_out: *mut *mut MoqGroupProducer,
) -> MoqResult {
    if track.is_null() || group_out.is_null() {
        return MoqResult::InvalidArgument;
    }

    let track = &*track;

    let group_id = next_id();
    let group_data = GroupProducerData {
        track_id: track.id,
        sequence,
        frames: Vec::new(),
        finished: false,
    };

    // Store the group data and update the track
    {
        let mut handles = HANDLES.lock().unwrap();
        handles.group_producers.insert(group_id, group_data);
        
        if let Some(track_data) = handles.track_producers.get_mut(&track.id) {
            track_data.groups.push(group_id);
        } else {
            return MoqResult::InvalidArgument;
        }
    }

    // Create and return the group handle
    let boxed_group = Box::new(MoqGroupProducer { id: group_id });
    *group_out = Box::into_raw(boxed_group);

    MoqResult::Success
}

/// Write a frame to a group producer
#[no_mangle]
pub unsafe extern "C" fn moq_group_producer_write_frame(
    group: *mut MoqGroupProducer,
    data: *const u8,
    data_len: usize,
) -> MoqResult {
    if group.is_null() || (data.is_null() && data_len > 0) {
        return MoqResult::InvalidArgument;
    }

    let group = &*group;

    let frame_data = if data_len == 0 {
        Vec::new()
    } else {
        std::slice::from_raw_parts(data, data_len).to_vec()
    };

    // Add the frame to the group
    {
        let mut handles = HANDLES.lock().unwrap();
        if let Some(group_data) = handles.group_producers.get_mut(&group.id) {
            if group_data.finished {
                return MoqResult::InvalidArgument; // Can't write to finished group
            }
            group_data.frames.push(frame_data);
        } else {
            return MoqResult::InvalidArgument;
        }
    }

    MoqResult::Success
}

/// Finish a group producer
#[no_mangle]
pub unsafe extern "C" fn moq_group_producer_finish(group: *mut MoqGroupProducer) -> MoqResult {
    if group.is_null() {
        return MoqResult::InvalidArgument;
    }

    let group = &*group;

    // Mark the group as finished
    {
        let mut handles = HANDLES.lock().unwrap();
        if let Some(group_data) = handles.group_producers.get_mut(&group.id) {
            group_data.finished = true;
        } else {
            return MoqResult::InvalidArgument;
        }
    }

    MoqResult::Success
}

/// Get the next group from a track consumer (blocking simulation)
#[no_mangle]
pub unsafe extern "C" fn moq_track_consumer_next_group(
    track: *mut MoqTrackConsumer,
    group_out: *mut *mut MoqGroupConsumer,
) -> MoqResult {
    if track.is_null() || group_out.is_null() {
        return MoqResult::InvalidArgument;
    }

    let track = &*track;

    // For now, return null to simulate no more groups
    // In a real implementation, this would wait for the next group
    *group_out = ptr::null_mut();
    
    MoqResult::Success
}

/// Read a frame from a group consumer
#[no_mangle]
pub unsafe extern "C" fn moq_group_consumer_read_frame(
    group: *mut MoqGroupConsumer,
    data_out: *mut *mut u8,
    data_len_out: *mut usize,
) -> MoqResult {
    if group.is_null() || data_out.is_null() || data_len_out.is_null() {
        return MoqResult::InvalidArgument;
    }

    // For now, return null to simulate no more frames
    // In a real implementation, this would return the next frame
    *data_out = ptr::null_mut();
    *data_len_out = 0;
    
    MoqResult::Success
}

/// Free memory allocated by the FFI layer
#[no_mangle]
pub unsafe extern "C" fn moq_free(ptr: *mut u8) {
    if !ptr.is_null() {
        // This would properly free memory allocated by the FFI layer
        // For now, it's a no-op since we're not allocating frame data
    }
}

// Free functions for all handle types
#[no_mangle]
pub unsafe extern "C" fn moq_broadcast_producer_free(producer: *mut MoqBroadcastProducer) {
    if !producer.is_null() {
        let producer = Box::from_raw(producer);
        let mut handles = HANDLES.lock().unwrap();
        handles.broadcast_producers.remove(&producer.id);
    }
}

#[no_mangle]
pub unsafe extern "C" fn moq_broadcast_consumer_free(consumer: *mut MoqBroadcastConsumer) {
    if !consumer.is_null() {
        let consumer = Box::from_raw(consumer);
        let mut handles = HANDLES.lock().unwrap();
        handles.broadcast_consumers.remove(&consumer.id);
    }
}

#[no_mangle]
pub unsafe extern "C" fn moq_track_producer_free(track: *mut MoqTrackProducer) {
    if !track.is_null() {
        let track = Box::from_raw(track);
        let mut handles = HANDLES.lock().unwrap();
        handles.track_producers.remove(&track.id);
    }
}

#[no_mangle]
pub unsafe extern "C" fn moq_track_consumer_free(track: *mut MoqTrackConsumer) {
    if !track.is_null() {
        let track = Box::from_raw(track);
        let mut handles = HANDLES.lock().unwrap();
        handles.track_consumers.remove(&track.id);
    }
}

#[no_mangle]
pub unsafe extern "C" fn moq_group_producer_free(group: *mut MoqGroupProducer) {
    if !group.is_null() {
        let group = Box::from_raw(group);
        let mut handles = HANDLES.lock().unwrap();
        handles.group_producers.remove(&group.id);
    }
}

#[no_mangle]
pub unsafe extern "C" fn moq_group_consumer_free(group: *mut MoqGroupConsumer) {
    if !group.is_null() {
        let group = Box::from_raw(group);
        let mut handles = HANDLES.lock().unwrap();
        handles.group_consumers.remove(&group.id);
    }
}

/// Get the last error message (placeholder)
#[no_mangle]
pub extern "C" fn moq_get_last_error() -> *const c_char {
    ptr::null()
}

/// Convert a MoqResult to a human-readable string
#[no_mangle]
pub extern "C" fn moq_result_to_string(result: MoqResult) -> *const c_char {
    match result {
        MoqResult::Success => b"Success\0".as_ptr() as *const c_char,
        MoqResult::InvalidArgument => b"Invalid argument\0".as_ptr() as *const c_char,
        MoqResult::NetworkError => b"Network error\0".as_ptr() as *const c_char,
        MoqResult::TlsError => b"TLS error\0".as_ptr() as *const c_char,
        MoqResult::DnsError => b"DNS error\0".as_ptr() as *const c_char,
        MoqResult::GeneralError => b"General error\0".as_ptr() as *const c_char,
    }
}
