#![allow(clippy::missing_safety_doc)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::collections::HashMap;
use std::ffi::CStr;
use std::os::raw::c_char;
use std::ptr;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;
use tokio::runtime::Runtime;
use url::Url;

// Import MOQ libraries
use moq_lite::*;
use moq_native::web_transport_quinn;

/// Global async runtime for handling MOQ operations
static RUNTIME: LazyLock<Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to create Tokio runtime")
});

/// Global storage for all MOQ handles
static HANDLES: LazyLock<Mutex<HandleStorage>> = LazyLock::new(|| Mutex::new(HandleStorage::new()));

/// Global storage for allocated memory pointers and their sizes
static MEMORY_TRACKER: LazyLock<Mutex<HashMap<usize, usize>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

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
    client: moq_native::Client,
}

/// Session data
#[allow(dead_code)]
struct SessionData {
    client_id: u64,
    url: String,
    session: moq_lite::Session<web_transport_quinn::Session>,
    publish_origin: Option<moq_lite::OriginProducer>,
    subscribe_origin: Option<moq_lite::OriginConsumer>,
}

/// Broadcast producer data
struct BroadcastProducerData {
    name: String,
    broadcast: moq_lite::Produce<moq_lite::BroadcastProducer, moq_lite::BroadcastConsumer>,
    tracks: Vec<u64>, // Track producer IDs
}

/// Broadcast consumer data
#[allow(dead_code)]
struct BroadcastConsumerData {
    session_id: u64,
    name: String,
    consumer: BroadcastConsumer,
    tracks: Vec<u64>, // Track consumer IDs
}

/// Track producer data
#[allow(dead_code)]
struct TrackProducerData {
    broadcast_id: u64,
    name: String,
    priority: u8,
    producer: TrackProducer,
    groups: Vec<u64>, // Group producer IDs
}

/// Track consumer data
#[allow(dead_code)]
struct TrackConsumerData {
    session_id: u64, // Track which session this consumer belongs to
    broadcast_id: u64,
    name: String,
    priority: u8,
    consumer: TrackConsumer,
    groups: Vec<u64>, // Group consumer IDs
}

/// Group producer data
#[allow(dead_code)]
struct GroupProducerData {
    track_id: u64,
    sequence: u64,
    producer: GroupProducer,
    finished: bool,
}

/// Group consumer data
#[allow(dead_code)]
struct GroupConsumerData {
    session_id: u64, // Track which session this consumer belongs to
    track_id: u64,
    sequence: u64,
    consumer: GroupConsumer,
    current_frame: usize,
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

/// Session modes for MOQ connections
#[repr(C)]
#[derive(Debug, PartialEq)]
pub enum MoqSessionMode {
    PublishOnly = 0,
    SubscribeOnly = 1,
    Both = 2,
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
    pub priority: u8,
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
    LazyLock::force(&MEMORY_TRACKER);
    LazyLock::force(&ID_COUNTER);

    MoqResult::Success
}

/// Create a new MOQ client
///
/// # Safety
/// This function dereferences raw pointers and must be called with valid pointers.
/// The `config` pointer must point to a valid MoqClientConfig struct, and
/// `client_out` must point to a valid location to store the client pointer.
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
        None
    } else {
        match CStr::from_ptr(config.tls_root_cert_path).to_str() {
            Ok(path) => Some(path.to_string()),
            Err(_) => return MoqResult::InvalidArgument,
        }
    };

    // Create MOQ native client config
    #[allow(clippy::field_reassign_with_default)]
    let client_config = {
        let mut moq_config = moq_native::ClientConfig::default();
        moq_config.bind = bind_addr
            .parse()
            .unwrap_or_else(|_| "[::]:0".parse().unwrap());
        moq_config.tls.disable_verify = Some(config.tls_disable_verify);
        if let Some(cert_path) = tls_root_cert_path {
            moq_config.tls.root = vec![cert_path.into()];
        }
        moq_config
    };

    // Initialize the client
    let client = match RUNTIME.block_on(async { client_config.init() }) {
        Ok(client) => client,
        Err(_) => return MoqResult::GeneralError,
    };

    let client_id = next_id();
    let client_data = ClientData { client };

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

    let url = match Url::parse(url_str) {
        Ok(url) => url,
        Err(_) => return MoqResult::InvalidArgument,
    };

    // Get the client and establish connection
    let (session_data, session_created) = {
        let mut handles = HANDLES.lock().unwrap();
        if let Some(client_data) = handles.clients.get_mut(&client.id) {
            match RUNTIME.block_on(async {
                let connection = client_data.client.connect(url.clone()).await?;

                // Create origins for publish/subscribe
                let publish_origin = moq_lite::Origin::produce();
                let session =
                    moq_lite::Session::connect(connection, Some(publish_origin.consumer), None)
                        .await?;

                Ok::<_, anyhow::Error>((session, publish_origin.producer))
            }) {
                Ok((session, publish_origin)) => {
                    let session_data = SessionData {
                        client_id: client.id,
                        url: url_str.to_string(),
                        session,
                        publish_origin: Some(publish_origin),
                        subscribe_origin: None,
                    };
                    (session_data, true)
                }
                Err(_) => return MoqResult::NetworkError,
            }
        } else {
            return MoqResult::InvalidArgument;
        }
    };

    let session_id = next_id();

    // Store the session data
    {
        let mut handles = HANDLES.lock().unwrap();
        if session_created {
            handles.sessions.insert(session_id, session_data);
        }
    }

    // Create and return the session handle
    let boxed_session = Box::new(MoqSession { id: session_id });
    *session_out = Box::into_raw(boxed_session);

    MoqResult::Success
}

/// Connect to a MOQ server with specified session mode
#[no_mangle]
pub unsafe extern "C" fn moq_client_connect_with_mode(
    client: *mut MoqClient,
    url: *const c_char,
    mode: MoqSessionMode,
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

    let url = match Url::parse(url_str) {
        Ok(url) => url,
        Err(_) => return MoqResult::InvalidArgument,
    };

    // Get the client and establish connection
    let (session_data, session_created) = {
        let mut handles = HANDLES.lock().unwrap();
        if let Some(client_data) = handles.clients.get_mut(&client.id) {
            match RUNTIME.block_on(async {
                let connection = client_data.client.connect(url.clone()).await?;

                // Create origins based on session mode
                let (session, publish_origin_producer, subscribe_origin_consumer) = match mode {
                    MoqSessionMode::PublishOnly => {
                        let publish_origin = moq_lite::Origin::produce();
                        let producer = publish_origin.producer;
                        let consumer = publish_origin.consumer;
                        let session =
                            moq_lite::Session::connect(connection, Some(consumer), None).await?;
                        (session, Some(producer), None)
                    }
                    MoqSessionMode::SubscribeOnly => {
                        let subscribe_origin = moq_lite::Origin::produce();
                        let producer = subscribe_origin.producer;
                        let consumer = subscribe_origin.consumer;
                        let session =
                            moq_lite::Session::connect(connection, None, Some(producer)).await?;
                        (session, None, Some(consumer))
                    }
                    MoqSessionMode::Both => {
                        let publish_origin = moq_lite::Origin::produce();
                        let subscribe_origin = moq_lite::Origin::produce();
                        let pub_producer = publish_origin.producer;
                        let pub_consumer = publish_origin.consumer;
                        let sub_producer = subscribe_origin.producer;
                        let sub_consumer = subscribe_origin.consumer;
                        let session = moq_lite::Session::connect(
                            connection,
                            Some(pub_consumer),
                            Some(sub_producer),
                        )
                        .await?;
                        (session, Some(pub_producer), Some(sub_consumer))
                    }
                };

                Ok::<_, anyhow::Error>((
                    session,
                    publish_origin_producer,
                    subscribe_origin_consumer,
                ))
            }) {
                Ok((session, publish_origin, subscribe_origin)) => {
                    let session_data = SessionData {
                        client_id: client.id,
                        url: url_str.to_string(),
                        session,
                        publish_origin,
                        subscribe_origin,
                    };
                    (session_data, true)
                }
                Err(_) => return MoqResult::NetworkError,
            }
        } else {
            return MoqResult::InvalidArgument;
        }
    };

    let session_id = next_id();

    // Store the session data
    {
        let mut handles = HANDLES.lock().unwrap();
        if session_created {
            handles.sessions.insert(session_id, session_data);
        }
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

    // If the session exists in our handle storage, consider it connected
    handles.sessions.contains_key(&session.id)
}

/// Close a MOQ session
#[no_mangle]
pub unsafe extern "C" fn moq_session_close(session: *mut MoqSession) -> MoqResult {
    if session.is_null() {
        return MoqResult::InvalidArgument;
    }

    let session = &*session;
    let mut handles = HANDLES.lock().unwrap();

    if handles.sessions.remove(&session.id).is_some() {
        MoqResult::Success
    } else {
        MoqResult::InvalidArgument
    }
}

/// Check if a MOQ session is still alive (blocking call)
/// Returns true if session is alive, false if closed/terminated
/// This is a blocking call that will check the session state
#[no_mangle]
pub unsafe extern "C" fn moq_session_is_alive(session: *const MoqSession) -> bool {
    if session.is_null() {
        return false;
    }

    let session = &*session;
    let handles = HANDLES.lock().unwrap();

    if let Some(session_data) = handles.sessions.get(&session.id) {
        // Create a clone of the session to check its state
        // This is a blocking operation that checks if the session is closed
        let session_closed_future = session_data.session.closed();

        // Use runtime to block on the future with a very short timeout
        // If it times out, the session is still alive; if it completes, it's closed
        match RUNTIME.block_on(async {
            tokio::time::timeout(Duration::from_millis(1), session_closed_future).await
        }) {
            Ok(_) => false, // Session closed future completed - session is closed
            Err(_) => true, // Timeout - session is still alive
        }
    } else {
        false
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
    let broadcast_produce = moq_lite::Broadcast::produce();
    let producer_data = BroadcastProducerData {
        name: String::new(),
        broadcast: broadcast_produce,
        tracks: Vec::new(),
    };

    // Store the producer data
    {
        let mut handles = HANDLES.lock().unwrap();
        handles
            .broadcast_producers
            .insert(producer_id, producer_data);
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

    // Create the real MOQ track
    let moq_track = Track {
        name: track_name.clone(),
        priority: track_info.priority,
    };

    let track_producer = {
        let mut handles = HANDLES.lock().unwrap();
        if let Some(_broadcast) = handles.broadcast_producers.get_mut(&producer.id) {
            // Need to create the track within the Tokio runtime context
            // Drop the mutex temporarily to avoid deadlock during block_on

            // Clone the track for use in the async block
            let track_clone = moq_track.clone();

            // We need to handle this differently since create() spawns tasks
            // For now, let's try to call it directly and see if we can make it work
            drop(handles); // Release the mutex before the potentially blocking operation

            // Use the runtime to ensure we're in the right context
            let track_result = RUNTIME.block_on(async {
                // Re-acquire the lock inside the async block
                let mut handles = HANDLES.lock().unwrap();
                if let Some(broadcast) = handles.broadcast_producers.get_mut(&producer.id) {
                    Ok(broadcast.broadcast.producer.create_track(track_clone))
                } else {
                    Err(())
                }
            });

            match track_result {
                Ok(producer) => producer,
                Err(_) => return MoqResult::InvalidArgument,
            }
        } else {
            return MoqResult::InvalidArgument;
        }
    };

    let track_data = TrackProducerData {
        broadcast_id: producer.id,
        name: track_name,
        priority: track_info.priority,
        producer: track_producer,
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

    // Publish the broadcast on the session
    {
        let mut handles = HANDLES.lock().unwrap();

        // Validate session exists
        if !handles.sessions.contains_key(&session.id) {
            return MoqResult::InvalidArgument;
        }

        // Get the broadcast consumer
        let consumer = if let Some(broadcast) = handles.broadcast_producers.get_mut(&producer.id) {
            broadcast.name = name.clone();
            broadcast.broadcast.consumer.clone()
        } else {
            return MoqResult::InvalidArgument;
        };

        // Clone the data we need and drop the lock to avoid deadlock
        let session_id = session.id;
        drop(handles);

        // Use the runtime to ensure we're in the right context for any async operations
        RUNTIME.block_on(async {
            let mut handles = HANDLES.lock().unwrap();
            if let Some(session_data) = handles.sessions.get_mut(&session_id) {
                // Use the origin producer to publish the broadcast
                if let Some(ref mut origin_producer) = session_data.publish_origin {
                    origin_producer.publish_broadcast(&name, consumer);
                }
            }
        });
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

    // Block on the async operation directly
    let consumer = {
        let mut handles = HANDLES.lock().unwrap();
        if let Some(session_data) = handles.sessions.get_mut(&session.id) {
            // Use the origin consumer to consume broadcasts
            if let Some(ref origin_consumer) = session_data.subscribe_origin {
                // Wrap in block_on in case consume_broadcast internally uses async operations
                RUNTIME.block_on(async { origin_consumer.consume_broadcast(&name) })
            } else {
                None
            }
        } else {
            None
        }
    };

    let consumer = match consumer {
        Some(c) => c,
        None => return MoqResult::InvalidArgument,
    };

    let consumer_id = next_id();
    let consumer_data = BroadcastConsumerData {
        session_id: session.id,
        name,
        consumer,
        tracks: Vec::new(),
    };

    // Store the consumer data
    {
        let mut handles = HANDLES.lock().unwrap();
        handles
            .broadcast_consumers
            .insert(consumer_id, consumer_data);
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

    // Create the real MOQ track and subscribe
    let moq_track = Track {
        name: track_name.clone(),
        priority: track_info.priority,
    };

    let track_consumer = {
        let mut handles = HANDLES.lock().unwrap();
        if let Some(broadcast) = handles.broadcast_consumers.get_mut(&consumer.id) {
            // Wrap in block_on in case subscribe_track internally uses async operations
            RUNTIME.block_on(async { broadcast.consumer.subscribe_track(&moq_track) })
        } else {
            return MoqResult::InvalidArgument;
        }
    };

    // Get the session_id from the broadcast consumer
    let session_id = {
        let handles = HANDLES.lock().unwrap();
        if let Some(broadcast) = handles.broadcast_consumers.get(&consumer.id) {
            broadcast.session_id
        } else {
            return MoqResult::InvalidArgument;
        }
    };

    let track_data = TrackConsumerData {
        session_id,
        broadcast_id: consumer.id,
        name: track_name,
        priority: track_info.priority,
        consumer: track_consumer,
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

    // Create the real MOQ group
    let group_producer = {
        let mut handles = HANDLES.lock().unwrap();
        if let Some(track_data) = handles.track_producers.get_mut(&track.id) {
            let group_info = moq_lite::Group { sequence };
            match track_data.producer.create_group(group_info) {
                Some(group) => group,
                None => return MoqResult::GeneralError,
            }
        } else {
            return MoqResult::InvalidArgument;
        }
    };

    let group_data = GroupProducerData {
        track_id: track.id,
        sequence,
        producer: group_producer,
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

    // Write the frame to the real MOQ group
    let result = {
        let mut handles = HANDLES.lock().unwrap();
        if let Some(group_data) = handles.group_producers.get_mut(&group.id) {
            if group_data.finished {
                return MoqResult::InvalidArgument; // Can't write to finished group
            }

            // write_frame returns () in moq-lite
            group_data.producer.write_frame(frame_data);
            MoqResult::Success
        } else {
            MoqResult::InvalidArgument
        }
    };

    result
}

/// Finish a group producer
#[no_mangle]
pub unsafe extern "C" fn moq_group_producer_finish(group: *mut MoqGroupProducer) -> MoqResult {
    if group.is_null() {
        return MoqResult::InvalidArgument;
    }

    let group = &*group;

    // Finish the real MOQ group
    {
        let mut handles = HANDLES.lock().unwrap();
        if let Some(group_data) = handles.group_producers.remove(&group.id) {
            // finish takes ownership, so we need to remove the producer from the map
            group_data.producer.close();
            MoqResult::Success
        } else {
            MoqResult::InvalidArgument
        }
    }
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

    // Get the next group using proper async handling
    let group_consumer_opt = {
        let track_id = track.id;

        // Temporarily remove the consumer from the map to avoid holding the mutex
        // during the blocking async operation
        let (mut track_data, session_id) = {
            let mut handles = HANDLES.lock().unwrap();
            if let Some(track_data) = handles.track_consumers.remove(&track_id) {
                // Check if the session still exists
                if !handles.sessions.contains_key(&track_data.session_id) {
                    // Session was closed, return immediately with null group
                    // Restore the track consumer first
                    handles.track_consumers.insert(track_id, track_data);
                    drop(handles);
                    *group_out = ptr::null_mut();
                    return MoqResult::Success;
                }
                let session_id = track_data.session_id;
                // Take ownership of the entire track_data - don't clone the consumer
                (track_data, session_id)
            } else {
                return MoqResult::InvalidArgument;
            }
        }; // MutexGuard is dropped here

        // Now call the async operation with timeout loop to check if session closes
        let result = loop {
            // Check if session still exists before waiting
            {
                let handles = HANDLES.lock().unwrap();
                if !handles.sessions.contains_key(&session_id) {
                    // Session was closed while we were waiting
                    break None;
                }
            }

            // Try to get next group with a short timeout
            // Work directly with the consumer in track_data to maintain state
            match RUNTIME.block_on(async {
                tokio::time::timeout(
                    tokio::time::Duration::from_millis(500),
                    track_data.consumer.next_group(),
                )
                .await
            }) {
                Ok(Ok(Some(group))) => {
                    break Some((group, session_id)); // Return both group and session_id
                }
                Ok(Ok(None)) => break None, // Stream ended
                Ok(Err(_)) => break None,   // Error
                Err(_) => {
                    // Timeout - loop back to check session status
                    continue;
                }
            }
        };

        // Restore the track_data back to the map before returning
        {
            let mut handles = HANDLES.lock().unwrap();
            handles.track_consumers.insert(track_id, track_data);
        }

        result
    };

    match group_consumer_opt {
        Some((group_consumer, session_id)) => {
            let group_id = next_id();
            let sequence = 0; // TODO: Get actual sequence from group

            let group_data = GroupConsumerData {
                session_id,
                track_id: track.id,
                sequence,
                consumer: group_consumer,
                current_frame: 0,
            };

            // Store the group data and update the track
            {
                let mut handles = HANDLES.lock().unwrap();
                handles.group_consumers.insert(group_id, group_data);

                if let Some(track_data) = handles.track_consumers.get_mut(&track.id) {
                    track_data.groups.push(group_id);
                }
            }

            // Create and return the group handle
            let boxed_group = Box::new(MoqGroupConsumer { id: group_id });
            *group_out = Box::into_raw(boxed_group);

            MoqResult::Success
        }
        None => {
            // No groups available
            *group_out = ptr::null_mut();
            MoqResult::Success
        }
    }
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

    let group = &*group;

    // Get the next frame using proper async handling with timeout loop
    let frame_data_opt = {
        let group_id = group.id;

        // Take ownership of the consumer temporarily and get session_id
        let (mut consumer_temp, session_id) = {
            let mut handles = HANDLES.lock().unwrap();
            match handles.group_consumers.remove(&group_id) {
                Some(data) => {
                    let session_id = data.session_id;
                    (data, session_id)
                }
                None => return MoqResult::InvalidArgument,
            }
        }; // MutexGuard is dropped here

        // Call read_frame with timeout loop to check if session closes
        let frame_opt = loop {
            // Check if session still exists before waiting
            {
                let handles = HANDLES.lock().unwrap();
                if !handles.sessions.contains_key(&session_id) {
                    // Session was closed while we were waiting
                    break None;
                }
            }

            // Try to read frame with a short timeout
            match RUNTIME.block_on(async {
                tokio::time::timeout(
                    tokio::time::Duration::from_millis(500),
                    consumer_temp.consumer.read_frame(),
                )
                .await
            }) {
                Ok(Ok(Some(frame))) => {
                    consumer_temp.current_frame += 1;
                    break Some(frame);
                }
                Ok(Ok(None)) => break None, // No more frames
                Ok(Err(_)) => break None,   // Error
                Err(_) => {
                    // Timeout - loop back to check session status
                    continue;
                }
            }
        };

        // Put the consumer back into the map
        {
            let mut handles = HANDLES.lock().unwrap();
            handles.group_consumers.insert(group_id, consumer_temp);
        }

        frame_opt
    };
    match frame_data_opt {
        Some(frame_bytes) => {
            let frame_data = frame_bytes.to_vec(); // Convert Bytes to Vec<u8>
            let len = frame_data.len();
            if len == 0 {
                *data_out = ptr::null_mut();
                *data_len_out = 0;
            } else {
                let layout = std::alloc::Layout::from_size_align(len, 1).unwrap();
                let ptr = std::alloc::alloc(layout);
                if ptr.is_null() {
                    return MoqResult::GeneralError;
                }

                std::ptr::copy_nonoverlapping(frame_data.as_ptr(), ptr, len);

                // Track the allocation
                {
                    let mut tracker = MEMORY_TRACKER.lock().unwrap();
                    tracker.insert(ptr as usize, len);
                }

                *data_out = ptr;
                *data_len_out = len;
            }

            MoqResult::Success
        }
        None => {
            // No frames available
            *data_out = ptr::null_mut();
            *data_len_out = 0;
            MoqResult::Success
        }
    }
}

/// Free memory allocated by the FFI layer
#[no_mangle]
pub unsafe extern "C" fn moq_free(ptr: *mut u8) {
    if !ptr.is_null() {
        let mut tracker = MEMORY_TRACKER.lock().unwrap();
        if let Some(size) = tracker.remove(&(ptr as usize)) {
            let layout = std::alloc::Layout::from_size_align(size, 1).unwrap();
            std::alloc::dealloc(ptr, layout);
        }
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
    let message = match result {
        MoqResult::Success => "Success",
        MoqResult::InvalidArgument => "Invalid argument",
        MoqResult::NetworkError => "Network error",
        MoqResult::TlsError => "TLS error",
        MoqResult::DnsError => "DNS error",
        MoqResult::GeneralError => "General error",
    };

    message.as_ptr() as *const c_char
}

/// Spawn a long-running task that calls a C callback function
/// This ensures the task runs within the Tokio runtime context
#[no_mangle]
pub unsafe extern "C" fn moq_spawn_task(
    callback: extern "C" fn(*mut std::os::raw::c_void),
    user_data: *mut std::os::raw::c_void,
) -> MoqResult {
    // Convert the raw pointer to usize to make it Send
    let user_data_addr = user_data as usize;
    RUNTIME.spawn(async move {
        // Convert back to pointer inside the async block
        let user_data_ptr = user_data_addr as *mut std::os::raw::c_void;
        callback(user_data_ptr);
    });

    MoqResult::Success
}

/// Function to ensure all FFI functions are kept in the binary
/// This prevents dead code elimination of exported functions
#[no_mangle]
pub extern "C" fn _moq_ffi_keep_symbols() {
    // This function should never be called, but referencing the functions
    // ensures they are not eliminated by the compiler
    let _funcs = [
        moq_init as *const (),
        moq_client_new as *const (),
        moq_client_connect as *const (),
        moq_client_free as *const (),
        moq_session_free as *const (),
        moq_session_is_connected as *const (),
        moq_session_close as *const (),
        moq_broadcast_producer_new as *const (),
        moq_broadcast_producer_create_track as *const (),
        moq_session_publish as *const (),
        moq_session_consume as *const (),
        moq_broadcast_consumer_subscribe_track as *const (),
        moq_track_producer_create_group as *const (),
        moq_group_producer_write_frame as *const (),
        moq_group_producer_finish as *const (),
        moq_track_consumer_next_group as *const (),
        moq_group_consumer_read_frame as *const (),
        moq_free as *const (),
        moq_broadcast_producer_free as *const (),
        moq_broadcast_consumer_free as *const (),
        moq_track_producer_free as *const (),
        moq_track_consumer_free as *const (),
        moq_group_producer_free as *const (),
        moq_group_consumer_free as *const (),
        moq_get_last_error as *const (),
        moq_result_to_string as *const (),
    ];
}
