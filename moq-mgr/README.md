# MOQ Manager Library

A simplified Rust library for managing MoQ (Media over QUIC) connections, subscriptions, and broadcasts with automatic catalog support.

## Features

- **Session Management**: Connect/disconnect from MoQ servers with automatic reconnection
- **Catalog Support**: Automatically processes catalog tracks and only subscribes to available tracks
- **Producer Mode**: Publish broadcasts with multiple tracks
- **Consumer Mode**: Subscribe to tracks with automatic filtering based on catalog availability
- **C FFI**: Complete C/C++ integration via Foreign Function Interface

## Architecture

- `session.rs`: Core session management with connect/disconnect and announcement handling
- `catalog.rs`: JSON catalog processing supporting both standard and HANG formats
- `producer.rs`: Track publishing with group/frame management
- `consumer.rs`: Track subscription with async data callbacks
- `ffi.rs`: C FFI layer for C++ integration

## Usage (C++ Example)

```cpp
#include "moq-mgr.h"
#include <iostream>

void error_callback(const char* msg, void* user_data) {
    std::cerr << "Error: " << msg << std::endl;
}

void status_callback(const char* msg, void* user_data) {
    std::cout << "Status: " << msg << std::endl;
}

void data_callback(const uint8_t* data, size_t len, void* user_data) {
    std::cout << "Received " << len << " bytes" << std::endl;
    // Process received data here
}

int main() {
    // Initialize library
    moq_mgr_init();
    
    // Create consumer session
    auto* session = moq_mgr_session_create(
        "https://relay.moq.sesame-streams.com:4433",
        "my-broadcast",
        1,  // SubscribeOnly mode
        1   // Enable auto-reconnect
    );
    
    // Set callbacks
    moq_mgr_session_set_error_callback(session, error_callback, nullptr);
    moq_mgr_session_set_status_callback(session, status_callback, nullptr);
    
    // Add subscriptions (will only subscribe if track exists in catalog)
    moq_mgr_session_add_subscription(session, "video/hd", data_callback, nullptr);
    moq_mgr_session_add_subscription(session, "audio/data", data_callback, nullptr);
    
    // Start the session
    if (moq_mgr_session_start(session) == MoqMgrResult::Success) {
        std::cout << "Session started successfully" << std::endl;
        
        // Keep running...
        std::cin.get();
    }
    
    // Cleanup
    moq_mgr_session_stop(session);
    moq_mgr_session_destroy(session);
    
    return 0;
}
```

## Building

The library is built as part of the moq-cpp CMake project:

```bash
mkdir build && cd build
cmake ..
cmake --build .
```

Or build standalone:

```bash
cd moq-mgr
cargo build --release
```

## API Reference

### Session Creation
- `moq_mgr_init()`: Initialize library (call once at startup)
- `moq_mgr_session_create()`: Create a new session
- `moq_mgr_session_destroy()`: Destroy session and free resources

### Configuration
- `moq_mgr_session_set_error_callback()`: Set error notification callback
- `moq_mgr_session_set_status_callback()`: Set status notification callback
- `moq_mgr_session_add_subscription()`: Add track subscription (consumer mode)
- `moq_mgr_session_add_broadcast()`: Add track broadcast (producer mode)

### Control
- `moq_mgr_session_start()`: Connect to server and start session
- `moq_mgr_session_stop()`: Disconnect and stop session
- `moq_mgr_session_is_running()`: Check if session is active

## Catalog Support

The library automatically:
1. Subscribes to the `catalog.json` track
2. Parses catalog data (supports standard and HANG formats)
3. Only subscribes to tracks listed in the catalog
4. Automatically unsubscribes when tracks are removed from catalog

## Supported Catalog Formats

### Standard Format
```json
{
  "tracks": [
    {
      "trackName": "video/hd",
      "type": "video",
      "priority": 60
    },
    {
      "trackName": "audio/data",
      "type": "audio",
      "priority": 80
    }
  ]
}
```

### HANG Format (moq-clock style)
```json
{
  "video": {
    "priority": 60,
    "renditions": {
      "video/hd": {
        "bitrate": 1935361,
        "codec": "avc1.640028"
      }
    }
  },
  "audio": {
    "priority": 80,
    "renditions": {
      "audio/data": {
        "bitrate": 32000,
        "codec": "opus"
      }
    }
  }
}
```

## License

See LICENSE file in repository root.
