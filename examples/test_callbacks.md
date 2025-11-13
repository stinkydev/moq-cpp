# Testing the New Callback System

## What Changed

The MOQ C++ wrapper has been updated to remove reconnection logic and instead provide callbacks for key events:

### Removed Features:
- ❌ **Auto-reconnection** - Sessions no longer automatically reconnect when disconnected
- ❌ **Infinite retry loops** - Connection failures are reported once and the session stops

### New Features:
- ✅ **Broadcast Announced Callback** - Called when a broadcast becomes available/active
- ✅ **Broadcast Cancelled Callback** - Called when a broadcast is stopped or becomes unavailable  
- ✅ **Connection Closed Callback** - Called when the connection is closed (with reason)

## Updated Example

The `clock_subscriber.cpp` example now demonstrates these callbacks:

```cpp
// New callback functions
void BroadcastAnnouncedCallback(const std::string &path) {
    std::cout << "🟢 BROADCAST ANNOUNCED: " << path << std::endl;
}

void BroadcastCancelledCallback(const std::string &path) {
    std::cout << "🔴 BROADCAST CANCELLED: " << path << std::endl;
}

void ConnectionClosedCallback(const std::string &reason) {
    std::cout << "❌ CONNECTION CLOSED: " << reason << std::endl;
}

// Setup in session manager
session->SetBroadcastAnnouncedCallback(BroadcastAnnouncedCallback);
session->SetBroadcastCancelledCallback(BroadcastCancelledCallback);  
session->SetConnectionClosedCallback(ConnectionClosedCallback);
```

## Testing Instructions

1. **Build the examples:**
   ```bash
   cd examples/example-build
   cmake --build . --config Debug
   ```

2. **Run the subscriber:**
   ```bash
   ./Debug/clock_subscriber_example.exe
   ```

3. **Expected behavior:**
   - The application will attempt to connect once (no reconnection)
   - If a broadcast is found, you'll see: `🟢 BROADCAST ANNOUNCED: <path>`
   - If the broadcast stops, you'll see: `🔴 BROADCAST CANCELLED: <path>`
   - When connection closes, you'll see: `❌ CONNECTION CLOSED: <reason>`
   - The application will exit after connection closes (no reconnection attempts)

## API Changes

### C++ API:
```cpp
// New methods added to moq::Session class:
bool SetBroadcastAnnouncedCallback(const BroadcastAnnouncedCallback &callback);
bool SetBroadcastCancelledCallback(const BroadcastCancelledCallback &callback);
bool SetConnectionClosedCallback(const ConnectionClosedCallback &callback);

// New callback types:
using BroadcastAnnouncedCallback = std::function<void(const std::string &path)>;
using BroadcastCancelledCallback = std::function<void(const std::string &path)>;
using ConnectionClosedCallback = std::function<void(const std::string &reason)>;
```

### C FFI API:
```c
// New callback types:
typedef void (*CBroadcastAnnouncedCallback)(const char *path);
typedef void (*CBroadcastCancelledCallback)(const char *path); 
typedef void (*CConnectionClosedCallback)(const char *reason);

// New functions:
int moq_session_set_broadcast_announced_callback(void *session, CBroadcastAnnouncedCallback callback);
int moq_session_set_broadcast_cancelled_callback(void *session, CBroadcastCancelledCallback callback);
int moq_session_set_connection_closed_callback(void *session, CConnectionClosedCallback callback);
```

### Rust API:
```rust
// New callback types in MoqSession:
pub type BroadcastAnnouncedCallback = Box<dyn Fn(&str) + Send + Sync>;
pub type BroadcastCancelledCallback = Box<dyn Fn(&str) + Send + Sync>;  
pub type ConnectionClosedCallback = Box<dyn Fn(&str) + Send + Sync>;

// New methods:
async fn set_broadcast_announced_callback(&self, callback: BroadcastAnnouncedCallback);
async fn set_broadcast_cancelled_callback(&self, callback: BroadcastCancelledCallback);
async fn set_connection_closed_callback(&self, callback: ConnectionClosedCallback);
```

## Configuration Changes

- Removed `auto_reconnect` field from `SessionConfig`
- Updated connection logic to be single-attempt only
- Sessions now terminate gracefully when connection is lost