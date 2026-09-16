//! Sessions created through the C API share one runtime and one client.
//!
//! Kept in its own test binary so task counts on the shared runtime are not
//! disturbed by other tests. Tests in this file run one at a time.

use std::collections::HashMap;
use std::ffi::{c_char, CString};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use moq_wrapper::ffi::{
    moq_close_session, moq_create_publisher, moq_create_subscriber, moq_is_connected,
    moq_session_free, moq_session_set_data_callback, moq_write_frame, CCatalogType, CMoqSession,
    CTrackDefinitionFFI,
};
use moq_wrapper::shared_runtime;

fn serial() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn alive_tasks() -> usize {
    shared_runtime().unwrap().metrics().num_alive_tasks()
}

/// Waits until the shared runtime has at most `max` tasks alive.
fn wait_for_tasks_at_most(max: usize, within: Duration) -> usize {
    let deadline = Instant::now() + within;
    loop {
        let alive = alive_tasks();
        if alive <= max || Instant::now() >= deadline {
            return alive;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn tracks(names: &[CString]) -> Vec<CTrackDefinitionFFI> {
    names
        .iter()
        .enumerate()
        .map(|(i, name)| CTrackDefinitionFFI {
            name: name.as_ptr(),
            priority: i as u32 + 1,
            track_type: 0,
        })
        .collect()
}

unsafe fn subscriber(url: &str, broadcast: &str, names: &[CString]) -> *mut CMoqSession {
    let url = CString::new(url).unwrap();
    let broadcast = CString::new(broadcast).unwrap();
    let defs = tracks(names);
    moq_create_subscriber(
        url.as_ptr(),
        broadcast.as_ptr(),
        defs.as_ptr(),
        defs.len(),
        CCatalogType::Sesame,
        0,
    )
}

unsafe fn close_and_free(session: *mut CMoqSession) {
    moq_close_session(session);
    moq_session_free(session);
}

#[test]
fn freeing_unconnected_subscribers_stops_their_tasks() {
    let _serial = serial();
    // Nothing listens here, so every connect attempt stays pending until its timeout.
    let url = "https://127.0.0.1:9/anon";
    let names = [CString::new("video").unwrap()];

    unsafe {
        let warmup = subscriber(url, "warmup", &names);
        assert!(!warmup.is_null());
        moq_session_free(warmup);
    }
    let baseline = wait_for_tasks_at_most(1, Duration::from_secs(2));

    let sessions: Vec<_> = (0..8)
        .map(|i| unsafe { subscriber(url, &format!("broadcast-{i}"), &names) })
        .collect();
    assert!(sessions.iter().all(|s| !s.is_null()));
    assert!(alive_tasks() > baseline);

    // Free without Close: the connect is still pending and must be abandoned
    // well before the 5s connect timeout.
    for session in sessions {
        unsafe { moq_session_free(session) };
    }
    let alive = wait_for_tasks_at_most(baseline, Duration::from_secs(2));
    assert!(
        alive <= baseline,
        "{alive} tasks still alive after free, expected at most {baseline}"
    );
}

static RECEIVED: OnceLock<Mutex<HashMap<usize, usize>>> = OnceLock::new();

extern "C" fn count_frames(
    session: *mut CMoqSession,
    _track: *const c_char,
    _data: *const u8,
    _len: usize,
) {
    let mut received = RECEIVED
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    *received.entry(session as usize).or_default() += 1;
}

fn frames_received(session: *mut CMoqSession) -> usize {
    RECEIVED
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&(session as usize))
        .copied()
        .unwrap_or(0)
}

/// Several publishers and subscribers in one process against a live relay.
///
/// Set `MOQ_RELAY_URL`, for example `http://localhost:4443/anon` for a local moq-relay.
#[test]
#[ignore = "requires a live MoQ relay"]
fn live_many_sessions_share_runtime() {
    let _serial = serial();
    let relay = std::env::var("MOQ_RELAY_URL").expect("MOQ_RELAY_URL must be set");
    const COUNT: usize = 4;

    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let names = [CString::new("video").unwrap()];
    let url = CString::new(relay.as_str()).unwrap();

    let before = alive_tasks();

    let publishers: Vec<_> = (0..COUNT)
        .map(|i| {
            let broadcast = CString::new(format!("shared-runtime-{nonce}/{i}")).unwrap();
            let defs = tracks(&names);
            let session = unsafe {
                moq_create_publisher(
                    url.as_ptr(),
                    broadcast.as_ptr(),
                    defs.as_ptr(),
                    defs.len(),
                    CCatalogType::Sesame,
                )
            };
            assert!(!session.is_null(), "publisher {i} failed to connect");
            session
        })
        .collect();

    let subscribers: Vec<_> = (0..COUNT)
        .map(|i| unsafe {
            let session = subscriber(&relay, &format!("shared-runtime-{nonce}/{i}"), &names);
            assert!(!session.is_null());
            moq_session_set_data_callback(session, count_frames);
            session
        })
        .collect();

    let track = CString::new("video").unwrap();
    let payload = [0u8; 256];
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut frame = 0usize;
    while subscribers.iter().any(|s| frames_received(*s) < 5) {
        assert!(
            Instant::now() < deadline,
            "subscribers did not all receive data"
        );
        for publisher in &publishers {
            let rc = unsafe {
                moq_write_frame(
                    *publisher,
                    track.as_ptr(),
                    payload.as_ptr(),
                    payload.len(),
                    frame.is_multiple_of(10),
                )
            };
            assert_eq!(rc, 0);
        }
        frame += 1;
        std::thread::sleep(Duration::from_millis(20));
    }

    for session in publishers.iter().chain(subscribers.iter()) {
        assert_eq!(unsafe { moq_is_connected(*session) }, 1);
    }

    // Closing one pair leaves the rest running.
    unsafe {
        close_and_free(subscribers[0]);
        close_and_free(publishers[0]);
    }
    let received = frames_received(subscribers[1]);
    for _ in 0..50 {
        for publisher in &publishers[1..] {
            unsafe {
                moq_write_frame(
                    *publisher,
                    track.as_ptr(),
                    payload.as_ptr(),
                    payload.len(),
                    false,
                );
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(frames_received(subscribers[1]) > received);

    for session in publishers[1..].iter().chain(subscribers[1..].iter()) {
        unsafe { close_and_free(*session) };
    }
    let alive = wait_for_tasks_at_most(before + 1, Duration::from_secs(5));
    assert!(
        alive <= before + 1,
        "{alive} tasks still alive after closing all sessions, started with {before}"
    );
}
