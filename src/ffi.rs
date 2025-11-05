use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::ptr;
use std::sync::{Arc, Mutex};
use tokio::runtime::Runtime;
use tracing::Level;

use crate::{
    close_session, create_publisher, create_subscriber, init, set_data_callback, write_frame,
    write_single_frame, CatalogType, MoqSession, TrackDefinition, TrackType,
};

// Global storage for data callbacks
static mut DATA_CALLBACKS: Option<Mutex<HashMap<usize, CDataCallback>>> = None;

fn init_callbacks() {
    unsafe {
        if DATA_CALLBACKS.is_none() {
            DATA_CALLBACKS = Some(Mutex::new(HashMap::new()));
        }
    }
}

// Opaque handles for C API
pub struct CMoqSession {
    session: Arc<MoqSession>,
    runtime: Arc<Runtime>,
}

pub struct CTrackDefinition {
    name: String,
    priority: u32,
    track_type: TrackType,
}

// C-compatible enums
#[repr(C)]
pub enum CLogLevel {
    Trace = 0,
    Debug = 1,
    Info = 2,
    Warn = 3,
    Error = 4,
}

#[repr(C)]
pub enum CTrackType {
    Video = 0,
    Audio = 1,
    Data = 2,
}

#[repr(C)]
pub enum CCatalogType {
    None = 0,
    Sesame = 1,
    Hang = 2,
}

// Callback types
pub type CLogCallback = extern "C" fn(*const c_char, c_int, *const c_char);
pub type CDataCallback = extern "C" fn(*const c_char, *const u8, usize);

impl From<CLogLevel> for Level {
    fn from(level: CLogLevel) -> Self {
        match level {
            CLogLevel::Trace => Level::TRACE,
            CLogLevel::Debug => Level::DEBUG,
            CLogLevel::Info => Level::INFO,
            CLogLevel::Warn => Level::WARN,
            CLogLevel::Error => Level::ERROR,
        }
    }
}

impl From<CTrackType> for TrackType {
    fn from(track_type: CTrackType) -> Self {
        match track_type {
            CTrackType::Video => TrackType::Video,
            CTrackType::Audio => TrackType::Audio,
            CTrackType::Data => TrackType::Data,
        }
    }
}

impl From<u8> for CTrackType {
    fn from(value: u8) -> Self {
        match value {
            0 => CTrackType::Video,
            1 => CTrackType::Audio,
            2 => CTrackType::Data,
            _ => CTrackType::Data, // Default to Data for invalid values
        }
    }
}

impl From<CCatalogType> for CatalogType {
    fn from(catalog_type: CCatalogType) -> Self {
        match catalog_type {
            CCatalogType::None => CatalogType::None,
            CCatalogType::Sesame => CatalogType::Sesame,
            CCatalogType::Hang => CatalogType::Hang,
        }
    }
}

/// Initialize the MOQ library with logging
#[no_mangle]
pub extern "C" fn moq_init(log_level: CLogLevel, log_callback: Option<CLogCallback>) {
    let level = Level::from(log_level);

    if let Some(callback) = log_callback {
        let callback_wrapper = move |target: &str, level: Level, message: &str| {
            let target_cstr = CString::new(target).unwrap_or_default();
            let message_cstr = CString::new(message).unwrap_or_default();
            let level_int = match level {
                Level::TRACE => 0,
                Level::DEBUG => 1,
                Level::INFO => 2,
                Level::WARN => 3,
                Level::ERROR => 4,
            };
            callback(target_cstr.as_ptr(), level_int, message_cstr.as_ptr());
        };
        init(level, Some(Box::new(callback_wrapper)));
    } else {
        init(level, None);
    }
}

/// Create a new track definition
#[no_mangle]
pub unsafe extern "C" fn moq_track_definition_new(
    name: *const c_char,
    priority: u32,
    track_type: u8,
) -> *mut CTrackDefinition {
    if name.is_null() {
        return ptr::null_mut();
    }

    let name_str = match CStr::from_ptr(name).to_str() {
        Ok(s) => s.to_string(),
        Err(_) => return ptr::null_mut(),
    };

    let track_def = CTrackDefinition {
        name: name_str,
        priority,
        track_type: TrackType::from(CTrackType::from(track_type)),
    };

    Box::into_raw(Box::new(track_def))
}

/// Free a track definition
#[no_mangle]
pub unsafe extern "C" fn moq_track_definition_free(track_def: *mut CTrackDefinition) {
    if !track_def.is_null() {
        drop(Box::from_raw(track_def));
    }
}

/// Create a publisher session
#[no_mangle]
pub unsafe extern "C" fn moq_create_publisher(
    url: *const c_char,
    broadcast_name: *const c_char,
    tracks: *const *mut CTrackDefinition,
    track_count: usize,
    catalog_type: CCatalogType,
) -> *mut CMoqSession {
    if url.is_null() || broadcast_name.is_null() {
        return ptr::null_mut();
    }

    let url_str = unsafe {
        match CStr::from_ptr(url).to_str() {
            Ok(s) => s,
            Err(_) => return ptr::null_mut(),
        }
    };

    let broadcast_str = unsafe {
        match CStr::from_ptr(broadcast_name).to_str() {
            Ok(s) => s,
            Err(_) => return ptr::null_mut(),
        }
    };

    let track_defs = if !tracks.is_null() && track_count > 0 {
        let track_slice = unsafe { std::slice::from_raw_parts(tracks, track_count) };
        track_slice
            .iter()
            .filter_map(|&ptr| {
                if ptr.is_null() {
                    None
                } else {
                    let track_def = unsafe { &*ptr };
                    Some(TrackDefinition::new(
                        &track_def.name,
                        track_def.priority,
                        track_def.track_type.clone(),
                    ))
                }
            })
            .collect()
    } else {
        Vec::new()
    };

    let runtime = match Runtime::new() {
        Ok(rt) => Arc::new(rt),
        Err(_) => return ptr::null_mut(),
    };

    let session = match runtime.block_on(create_publisher(
        url_str,
        broadcast_str,
        track_defs,
        CatalogType::from(catalog_type),
    )) {
        Ok(s) => Arc::new(s),
        Err(_) => return ptr::null_mut(),
    };

    let c_session = CMoqSession { session, runtime };

    Box::into_raw(Box::new(c_session))
}

/// Create a subscriber session
#[no_mangle]
pub unsafe extern "C" fn moq_create_subscriber(
    url: *const c_char,
    broadcast_name: *const c_char,
    tracks: *const *mut CTrackDefinition,
    track_count: usize,
    catalog_type: CCatalogType,
) -> *mut CMoqSession {
    if url.is_null() || broadcast_name.is_null() {
        return ptr::null_mut();
    }

    let url_str = unsafe {
        match CStr::from_ptr(url).to_str() {
            Ok(s) => s,
            Err(_) => return ptr::null_mut(),
        }
    };

    let broadcast_str = unsafe {
        match CStr::from_ptr(broadcast_name).to_str() {
            Ok(s) => s,
            Err(_) => return ptr::null_mut(),
        }
    };

    let track_defs = if !tracks.is_null() && track_count > 0 {
        let track_slice = unsafe { std::slice::from_raw_parts(tracks, track_count) };
        track_slice
            .iter()
            .filter_map(|&ptr| {
                if ptr.is_null() {
                    None
                } else {
                    let track_def = unsafe { &*ptr };
                    Some(TrackDefinition::new(
                        &track_def.name,
                        track_def.priority,
                        track_def.track_type.clone(),
                    ))
                }
            })
            .collect()
    } else {
        Vec::new()
    };

    let runtime = match Runtime::new() {
        Ok(rt) => Arc::new(rt),
        Err(_) => return ptr::null_mut(),
    };

    let session = match runtime.block_on(create_subscriber(
        url_str,
        broadcast_str,
        track_defs,
        CatalogType::from(catalog_type),
    )) {
        Ok(s) => Arc::new(s),
        Err(_) => return ptr::null_mut(),
    };

    let c_session = CMoqSession { session, runtime };

    Box::into_raw(Box::new(c_session))
}

/// Set data callback for receiving data
#[no_mangle]
pub unsafe extern "C" fn moq_set_data_callback(
    session: *mut CMoqSession,
    callback: CDataCallback,
) -> c_int {
    if session.is_null() {
        return -1;
    }

    init_callbacks();

    let session_ref = unsafe { &*session };

    // Store the callback globally
    let session_id = session as usize;
    unsafe {
        if let Some(ref callbacks) = DATA_CALLBACKS {
            if let Ok(mut map) = callbacks.lock() {
                map.insert(session_id, callback);
            }
        }
    }

    // Set up the Rust callback that will call the C callback
    session_ref.runtime.block_on(set_data_callback(
        &session_ref.session,
        move |track: String, data: Vec<u8>| unsafe {
            if let Some(ref callbacks) = DATA_CALLBACKS {
                if let Ok(map) = callbacks.lock() {
                    if let Some(&callback) = map.get(&session_id) {
                        let track_cstr = CString::new(track).unwrap_or_default();
                        callback(track_cstr.as_ptr(), data.as_ptr(), data.len());
                    }
                }
            }
        },
    ));

    0
}

/// Write a single frame in its own group (convenience function)
/// This corresponds to lib.rs write_single_frame()
#[no_mangle]
pub unsafe extern "C" fn moq_write_single_frame(
    session: *mut CMoqSession,
    track_name: *const c_char,
    data: *const u8,
    data_len: usize,
) -> c_int {
    if session.is_null() || track_name.is_null() || data.is_null() {
        return -1;
    }

    let session_ref = unsafe { &*session };

    let track_str = unsafe {
        match CStr::from_ptr(track_name).to_str() {
            Ok(s) => s,
            Err(_) => return -1,
        }
    };

    let data_slice = unsafe { std::slice::from_raw_parts(data, data_len) };
    let data_vec = data_slice.to_vec();

    match session_ref.runtime.block_on(write_single_frame(
        &session_ref.session,
        track_str,
        data_vec,
    )) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// Write a frame to a track with optional new group creation
/// This corresponds to lib.rs write_frame()
#[no_mangle]
pub unsafe extern "C" fn moq_write_frame(
    session: *mut CMoqSession,
    track_name: *const c_char,
    data: *const u8,
    data_len: usize,
    new_group: bool,
) -> c_int {
    if session.is_null() || track_name.is_null() || data.is_null() {
        return -1;
    }

    let session_ref = unsafe { &*session };

    let track_str = unsafe {
        match CStr::from_ptr(track_name).to_str() {
            Ok(s) => s,
            Err(_) => return -1,
        }
    };

    let data_slice = unsafe { std::slice::from_raw_parts(data, data_len) };
    let data_vec = data_slice.to_vec();

    match session_ref.runtime.block_on(write_frame(
        &session_ref.session,
        track_str,
        data_vec,
        new_group,
    )) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// Check if session is connected
#[no_mangle]
pub unsafe extern "C" fn moq_is_connected(session: *mut CMoqSession) -> c_int {
    if session.is_null() {
        return 0;
    }

    let session_ref = unsafe { &*session };
    if session_ref
        .runtime
        .block_on(session_ref.session.is_connected())
    {
        1
    } else {
        0
    }
}

/// Close a session
#[no_mangle]
pub unsafe extern "C" fn moq_close_session(session: *mut CMoqSession) -> c_int {
    if session.is_null() {
        return -1;
    }

    let session_ref = unsafe { &*session };
    match session_ref
        .runtime
        .block_on(close_session(&session_ref.session))
    {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// Free a session
#[no_mangle]
pub unsafe extern "C" fn moq_session_free(session: *mut CMoqSession) {
    if !session.is_null() {
        // Clean up the callback
        let session_id = session as usize;
        unsafe {
            if let Some(ref callbacks) = DATA_CALLBACKS {
                if let Ok(mut map) = callbacks.lock() {
                    map.remove(&session_id);
                }
            }
        }

        unsafe {
            drop(Box::from_raw(session));
        }
    }
}
