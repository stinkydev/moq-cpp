use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::ptr;
use std::sync::{Arc, RwLock};
use tokio::runtime::Runtime;
use tracing::Level;

use crate::{
    close_session, create_publisher, create_subscriber, set_data_callback, set_log_level,
    write_frame, write_single_frame, CatalogType, MoqSession, TrackDefinition, TrackType,
};

// Opaque handles for C API
pub struct CMoqSession {
    session: Arc<MoqSession>,
    runtime: Arc<Runtime>,
    data_callback: Arc<RwLock<Option<CDataCallback>>>,
    broadcast_announced_callback: Arc<RwLock<Option<CBroadcastAnnouncedCallback>>>,
    broadcast_cancelled_callback: Arc<RwLock<Option<CBroadcastCancelledCallback>>>,
    connection_closed_callback: Arc<RwLock<Option<CConnectionClosedCallback>>>,
}

// C-compatible struct for passing track definitions
#[repr(C)]
pub struct CTrackDefinitionFFI {
    name: *const c_char,
    priority: u32,
    track_type: u8,
}

// Keep the old struct for backward compatibility
#[allow(dead_code)]
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

#[repr(C)]
pub enum MoqResult {
    Success = 0,
    InvalidArgument = 1,
    RuntimeError = 2,
}

// Callback types with session context
pub type CLogCallback = extern "C" fn(*const c_char, c_int, *const c_char);
pub type CDataCallback = extern "C" fn(*mut CMoqSession, *const c_char, *const u8, usize);

// New callback types for broadcast events and connection status
pub type CBroadcastAnnouncedCallback = extern "C" fn(*const c_char);
pub type CBroadcastCancelledCallback = extern "C" fn(*const c_char);
pub type CConnectionClosedCallback = extern "C" fn(*const c_char);

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

/// Set the global log level for internal library tracing (optional)
#[no_mangle]
pub extern "C" fn moq_set_log_level(log_level: CLogLevel, _log_callback: Option<CLogCallback>) {
    let level = Level::from(log_level);
    // Note: Log callbacks are now set on individual MoqSession instances
    // The log_callback parameter is ignored for backward compatibility
    set_log_level(level);
}

/// Create a new track definition
///
/// # Safety
///
/// This function is unsafe because it dereferences raw pointers passed from C.
/// The caller must ensure that `name` is a valid null-terminated C string.
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
///
/// # Safety
///
/// This function is unsafe because it takes ownership of a raw pointer.
/// The caller must ensure that `track_def` was previously allocated by `moq_track_definition_new`
/// and has not been freed before.
#[no_mangle]
pub unsafe extern "C" fn moq_track_definition_free(track_def: *mut CTrackDefinition) {
    if !track_def.is_null() {
        drop(Box::from_raw(track_def));
    }
}

/// Create a publisher session
///
/// # Safety
///
/// This function is unsafe because it dereferences raw pointers passed from C.
/// The caller must ensure that:
/// - `url` and `broadcast_name` are valid null-terminated C strings
/// - `tracks` is a valid array of `track_count` elements
/// - All track pointers in the array are valid
#[no_mangle]
pub unsafe extern "C" fn moq_create_publisher(
    url: *const c_char,
    broadcast_name: *const c_char,
    tracks: *const CTrackDefinitionFFI,
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
        println!("DEBUG: Processing {} tracks for publisher", track_count);
        let track_slice = unsafe { std::slice::from_raw_parts(tracks, track_count) };
        let mut result = Vec::new();
        for (i, track_ffi) in track_slice.iter().enumerate() {
            if track_ffi.name.is_null() {
                println!("DEBUG: Track {} has null name", i);
                continue;
            }

            let name_str = unsafe {
                match CStr::from_ptr(track_ffi.name).to_str() {
                    Ok(s) => s,
                    Err(_) => {
                        println!("DEBUG: Track {} has invalid UTF-8 name", i);
                        continue;
                    }
                }
            };

            println!(
                "DEBUG: Track {} name: '{}', priority: {}, type: {}",
                i, name_str, track_ffi.priority, track_ffi.track_type
            );

            result.push(TrackDefinition::new(
                name_str,
                track_ffi.priority,
                TrackType::from(CTrackType::from(track_ffi.track_type)),
            ));
        }
        result
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

    let c_session = CMoqSession {
        session,
        runtime,
        data_callback: Arc::new(RwLock::new(None)),
        broadcast_announced_callback: Arc::new(RwLock::new(None)),
        broadcast_cancelled_callback: Arc::new(RwLock::new(None)),
        connection_closed_callback: Arc::new(RwLock::new(None)),
    };

    Box::into_raw(Box::new(c_session))
}

/// Create a subscriber session
///
/// # Safety
///
/// This function is unsafe because it dereferences raw pointers passed from C.
/// The caller must ensure that:
/// - `url` and `broadcast_name` are valid null-terminated C strings
/// - `tracks` is a valid array of `track_count` elements
/// - All track pointers in the array are valid
#[no_mangle]
pub unsafe extern "C" fn moq_create_subscriber(
    url: *const c_char,
    broadcast_name: *const c_char,
    tracks: *const CTrackDefinitionFFI,
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
        println!("DEBUG: Processing {} tracks for subscriber", track_count);
        let track_slice = unsafe { std::slice::from_raw_parts(tracks, track_count) };
        let mut result = Vec::new();
        for (i, track_ffi) in track_slice.iter().enumerate() {
            if track_ffi.name.is_null() {
                println!("DEBUG: Track {} has null name", i);
                continue;
            }

            let name_str = unsafe {
                match CStr::from_ptr(track_ffi.name).to_str() {
                    Ok(s) => s,
                    Err(_) => {
                        println!("DEBUG: Track {} has invalid UTF-8 name", i);
                        continue;
                    }
                }
            };

            println!(
                "DEBUG: Track {} name: '{}', priority: {}, type: {}",
                i, name_str, track_ffi.priority, track_ffi.track_type
            );

            result.push(TrackDefinition::new(
                name_str,
                track_ffi.priority,
                TrackType::from(CTrackType::from(track_ffi.track_type)),
            ));
        }
        result
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

    let c_session = CMoqSession {
        session,
        runtime,
        data_callback: Arc::new(RwLock::new(None)),
        broadcast_announced_callback: Arc::new(RwLock::new(None)),
        broadcast_cancelled_callback: Arc::new(RwLock::new(None)),
        connection_closed_callback: Arc::new(RwLock::new(None)),
    };

    Box::into_raw(Box::new(c_session))
}

/// Set data callback for receiving data
///
/// # Safety
///
/// This function is unsafe because it dereferences the raw `session` pointer.
/// The caller must ensure that `session` is a valid pointer returned from
/// `moq_create_publisher` or `moq_create_subscriber`.
#[no_mangle]
pub unsafe extern "C" fn moq_session_set_data_callback(
    session: *mut CMoqSession,
    callback: CDataCallback,
) -> c_int {
    if session.is_null() {
        return -1;
    }

    let session_ref = unsafe { &*session };

    // Store the callback in the session structure
    if let Ok(mut cb) = session_ref.data_callback.write() {
        *cb = Some(callback);
    }

    // Set up the Rust callback that will call the C callback with session context
    let session_addr = session as usize; // Convert to usize for thread safety
    let data_callback_ref = session_ref.data_callback.clone();

    session_ref.runtime.block_on(set_data_callback(
        &session_ref.session,
        move |track: String, data: Vec<u8>| {
            if let Ok(cb_guard) = data_callback_ref.read() {
                if let Some(callback) = *cb_guard {
                    let track_cstr = CString::new(track).unwrap_or_default();
                    // Convert back to pointer for the callback
                    let session_ptr = session_addr as *mut CMoqSession;
                    callback(session_ptr, track_cstr.as_ptr(), data.as_ptr(), data.len());
                }
            }
        },
    ));

    0
}

/// Write a single frame in its own group (convenience function)
/// This corresponds to lib.rs write_single_frame()
///
/// # Safety
///
/// This function is unsafe because it dereferences raw pointers.
/// The caller must ensure that:
/// - `session` is a valid pointer to a CMoqSession
/// - `track_name` is a valid null-terminated C string
/// - `data` points to a valid buffer of at least `data_len` bytes
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
///
/// # Safety
///
/// This function is unsafe because it dereferences raw pointers.
/// The caller must ensure that:
/// - `session` is a valid pointer to a CMoqSession
/// - `track_name` is a valid null-terminated C string
/// - `data` points to a valid buffer of at least `data_len` bytes
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
///
/// # Safety
///
/// This function is unsafe because it dereferences the raw `session` pointer.
/// The caller must ensure that `session` is a valid pointer to a CMoqSession.
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
///
/// # Safety
///
/// This function is unsafe because it dereferences the raw `session` pointer.
/// The caller must ensure that `session` is a valid pointer to a CMoqSession.
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
///
/// # Safety
///
/// This function is unsafe because it takes ownership of a raw pointer.
/// Set a log callback for a specific session to receive session-specific log messages
///
/// # Safety
/// The caller must ensure that `session` was previously allocated by
/// `moq_create_publisher` or `moq_create_subscriber` and has not been freed before.
/// The callback function pointer must be valid for the lifetime of the session.
#[no_mangle]
pub unsafe extern "C" fn moq_session_set_log_callback(
    session: *mut CMoqSession,
    callback: Option<CLogCallback>,
) -> MoqResult {
    if session.is_null() {
        return MoqResult::InvalidArgument;
    }

    let session_ref = unsafe { &*session };

    if let Some(callback) = callback {
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

        session_ref.runtime.block_on(
            session_ref
                .session
                .set_log_callback(Some(Box::new(callback_wrapper))),
        );
        MoqResult::Success
    } else {
        session_ref
            .runtime
            .block_on(session_ref.session.set_log_callback(None));
        MoqResult::Success
    }
}

/// Free a MoQ session and its resources
///
/// Set broadcast announced callback
///
/// # Safety
/// The caller must ensure that `session` is a valid pointer returned from
/// `moq_create_publisher` or `moq_create_subscriber`.
#[no_mangle]
pub unsafe extern "C" fn moq_session_set_broadcast_announced_callback(
    session: *mut CMoqSession,
    callback: CBroadcastAnnouncedCallback,
) -> c_int {
    if session.is_null() {
        return MoqResult::InvalidArgument as c_int;
    }

    let session_ref = unsafe { &*session };

    // Store the C callback
    if let Ok(mut cb) = session_ref.broadcast_announced_callback.write() {
        *cb = Some(callback);
    }

    // Set up the Rust callback that will call the C callback
    let c_callback = session_ref.broadcast_announced_callback.clone();
    let rust_callback = Box::new(move |path: &str| {
        if let Ok(guard) = c_callback.read() {
            if let Some(cb) = *guard {
                let c_path = CString::new(path).unwrap_or_else(|_| CString::new("").unwrap());
                cb(c_path.as_ptr());
            }
        }
    });

    // Set the callback in the session
    session_ref.runtime.block_on(async {
        session_ref
            .session
            .set_broadcast_announced_callback(rust_callback)
            .await;
    });

    MoqResult::Success as c_int
}

/// Set broadcast cancelled callback
///
/// # Safety
/// The caller must ensure that `session` is a valid pointer returned from
/// `moq_create_publisher` or `moq_create_subscriber`.
#[no_mangle]
pub unsafe extern "C" fn moq_session_set_broadcast_cancelled_callback(
    session: *mut CMoqSession,
    callback: CBroadcastCancelledCallback,
) -> c_int {
    if session.is_null() {
        return MoqResult::InvalidArgument as c_int;
    }

    let session_ref = unsafe { &*session };

    // Store the C callback
    if let Ok(mut cb) = session_ref.broadcast_cancelled_callback.write() {
        *cb = Some(callback);
    }

    // Set up the Rust callback that will call the C callback
    let c_callback = session_ref.broadcast_cancelled_callback.clone();
    let rust_callback = Box::new(move |path: &str| {
        if let Ok(guard) = c_callback.read() {
            if let Some(cb) = *guard {
                let c_path = CString::new(path).unwrap_or_else(|_| CString::new("").unwrap());
                cb(c_path.as_ptr());
            }
        }
    });

    // Set the callback in the session
    session_ref.runtime.block_on(async {
        session_ref
            .session
            .set_broadcast_cancelled_callback(rust_callback)
            .await;
    });

    MoqResult::Success as c_int
}

/// Set connection closed callback
///
/// # Safety
/// The caller must ensure that `session` is a valid pointer returned from
/// `moq_create_publisher` or `moq_create_subscriber`.
#[no_mangle]
pub unsafe extern "C" fn moq_session_set_connection_closed_callback(
    session: *mut CMoqSession,
    callback: CConnectionClosedCallback,
) -> c_int {
    if session.is_null() {
        return MoqResult::InvalidArgument as c_int;
    }

    let session_ref = unsafe { &*session };

    // Store the C callback
    if let Ok(mut cb) = session_ref.connection_closed_callback.write() {
        *cb = Some(callback);
    }

    // Set up the Rust callback that will call the C callback
    let c_callback = session_ref.connection_closed_callback.clone();
    let rust_callback = Box::new(move |reason: &str| {
        if let Ok(guard) = c_callback.read() {
            if let Some(cb) = *guard {
                let c_reason = CString::new(reason).unwrap_or_else(|_| CString::new("").unwrap());
                cb(c_reason.as_ptr());
            }
        }
    });

    // Set the callback in the session
    session_ref.runtime.block_on(async {
        session_ref
            .session
            .set_connection_closed_callback(rust_callback)
            .await;
    });

    MoqResult::Success as c_int
}

/// # Safety
/// The caller must ensure that `session` was previously allocated by
/// `moq_create_publisher` or `moq_create_subscriber` and has not been freed before.
#[no_mangle]
pub unsafe extern "C" fn moq_session_free(session: *mut CMoqSession) {
    if !session.is_null() {
        // Clear all callbacks before dropping the session
        let session_ref = unsafe { &*session };
        if let Ok(mut cb) = session_ref.data_callback.write() {
            *cb = None;
        }
        if let Ok(mut cb) = session_ref.broadcast_announced_callback.write() {
            *cb = None;
        }
        if let Ok(mut cb) = session_ref.broadcast_cancelled_callback.write() {
            *cb = None;
        }
        if let Ok(mut cb) = session_ref.connection_closed_callback.write() {
            *cb = None;
        }

        unsafe {
            drop(Box::from_raw(session));
        }
    }
}
