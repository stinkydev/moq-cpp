# MOQ Wrapper Project

A wrapper library for MOQ (Media over QUIC) with simplified Rust and C++ APIs, announced broadcast discovery, and room-prefix subscriptions.

## Project Structure

```
/
├── src/                    # Rust source code
│   ├── lib.rs             # Simplified public API
│   ├── session.rs         # Session management and announce handling
│   ├── subscription.rs    # Resilient subscription handling
│   ├── catalog.rs         # Catalog management
│   ├── track.rs          # Track management
│   ├── config.rs         # Configuration types
│   └── ffi.rs            # Foreign Function Interface (C API)
├── cpp/                   # C++ wrapper
│   ├── include/           # C++ headers
│   │   └── moq_wrapper.h
│   ├── src/              # C++ implementation
│   │   └── moq_wrapper.cpp
│   └── README.md         # C++ specific documentation
├── examples/              # Example applications (separate CMake project)
│   ├── clock_example.rs      # Rust clock example with robust error handling
│   ├── hang_subscriber.rs    # Rust hang subscriber
│   ├── clock_publisher.cpp   # C++ clock publisher
│   ├── clock_subscriber.cpp  # C++ clock subscriber
│   ├── CMakeLists.txt        # Separate CMake project for examples
│   └── README.md             # Instructions for building examples
├── tests/                 # Test files
├── CMakeLists.txt        # C++ build configuration
└── Cargo.toml           # Rust build configuration
```

## Building

### Rust Library

The Rust library supports both regular library usage and FFI exports:

```bash
# Build as regular Rust library
cargo build --release

# Build with FFI exports (creates libmoq_wrapper.dylib/.so/.dll)
cargo build --release --features ffi
```

### C++ Library (moq-cpp)

The C++ library wraps the Rust FFI and provides a modern C++ API:

```bash
# Create build directory
mkdir build && cd build

# Configure (examples disabled by default)
cmake .. -DCMAKE_BUILD_TYPE=Release

# Build library
cmake --build .

# Install
cmake --install . --prefix /usr/local
```

### C++ Examples

The examples are now a separate CMake project that demonstrates how to use the installed library:

```bash
# First, build and install the library
mkdir build && cd build
cmake .. -DCMAKE_BUILD_TYPE=Release
cmake --build .
cmake --install . --prefix /usr/local

# Then build examples
cd ../examples
cmake -B build -DCMAKE_BUILD_TYPE=Release
cmake --build build
```

See `examples/README.md` for detailed instructions.

## Quick Start

### Rust API - Simplified Interface

```rust
use moq_wrapper::{create_publisher, set_log_level, write_frame, CatalogType, Level, TrackDefinition};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize with logging
    set_log_level(Level::INFO);
    
    // Create publisher with track definitions
    let tracks = vec![
        TrackDefinition::data("seconds", 0), // Simple data track
    ];
    
    let session = create_publisher(
        "https://r2.moq.sesame-streams.com:4433",
        "my-broadcast",
        tracks,
        CatalogType::None  // No catalog needed for simple use cases
    ).await?;
    
    // No manual connection waiting needed - session handles everything automatically
    
    // Write data with automatic group management
    write_frame(&session, "seconds", "2024-11-05 12:00:00".into(), true).await?; // new_group=true
    write_frame(&session, "seconds", "30".into(), false).await?; // add to current group
    
    // Handle errors at the application boundary and watch session events/callbacks.
    
    Ok(())
}
```

### C++ API - Clean Interface

```cpp
#include "moq_wrapper.h"
#include <iostream>

int main() {
    // Initialize with logging
    moq::SetLogLevel(moq::LogLevel::kInfo);
    
    // Create publisher with simple track definition
    std::vector<moq::TrackDefinition> tracks;
    tracks.emplace_back("seconds", 0, moq::TrackType::kData);
    
    auto session = moq::Session::CreatePublisher(
        "https://r2.moq.sesame-streams.com:4433",
        "my-broadcast",
        tracks,
        moq::CatalogType::kNone
    );
    
    if (session) {
        // Write data - session handles connection automatically
        std::string time_data = "2024-11-05 12:00:00";
        session->WriteFrame("seconds", time_data.data(), time_data.size(), true); // new_group=true
        
        std::string seconds_data = "30";
        session->WriteFrame("seconds", seconds_data.data(), seconds_data.size(), false); // add to group
        
        std::cout << "Data published successfully" << std::endl;
    }
    
    return 0;
}
```

## Features

### Core Features
- **Announce-Based Rooms**: Subscribe to every announced broadcast under a prefix
- **Simplified API**: Just two main functions - `write_frame()` and `write_single_frame()`
- **Bulletproof Error Handling**: All operations handle network interruptions gracefully
- **Simple Connection Management**: Sessions expose callbacks/events for connection and announce state
- **Track Auto-Creation**: Track producers are automatically created and managed
- **Session-Level Diagnostics**: Applications can react to disconnects, announces, and unannounces

### Rust Features  
- **Async/Await**: Full async support with Tokio runtime
- **Graceful Degradation**: Operations fail gracefully when the connection is unavailable
- **Clean API**: `write_frame(session, track, data, new_group)` - that's it!
- **Type Safety**: Strong typing for all MOQ concepts
- **Room Subscriptions**: `create_room_subscriber` follows announced broadcasts under a prefix

### C++ Features
- **Modern C++17**: RAII, smart pointers, and Google style guidelines  
- **Simplified Interface**: `WriteFrame(track, data, size, new_group)` and `WriteSingleFrame(track, data, size)`
- **Automatic Resource Management**: No manual cleanup needed
- **Cross-Platform**: Supports Windows, macOS, and Linux
- **Thread Safe**: Safe to use from multiple threads

### Automatic Features (No Code Required)
- ✅ **Connection establishment and management**
- ✅ **Track producer creation and recreation**  
- ✅ **Group management and cleanup**
- ✅ **Catalog publishing (when configured)**
- ✅ **Network interruption handling**
- ✅ **Announcement and unannouncement events**

## Examples

### Running Rust Examples

```bash
# Clock publisher
cargo run --example clock_example -- \
  --url https://r2.moq.sesame-streams.com:4433 \
  --broadcast my-clock \
  publish

# Clock subscriber
cargo run --example clock_example -- \
  --url https://r2.moq.sesame-streams.com:4433 \
  --broadcast my-clock \
  subscribe

# Room subscriber for multiple announced publishers
cargo run --example clock_example -- \
  --url https://r2.moq.sesame-streams.com:4433 \
  --broadcast my-room \
  subscribe --room

# Hang subscriber  
cargo run --example hang_subscriber \
  --url https://r2.moq.sesame-streams.com:4433 \
  --broadcast hang-broadcast
```

### Running C++ Examples

```bash
# Build and install the library, then build examples
cmake -B build -DCMAKE_BUILD_TYPE=Release
cmake --build build --config Release
cmake --install build --prefix install
cmake -B build-examples -S examples -DCMAKE_PREFIX_PATH=install
cmake --build build-examples --config Release

# Clock publisher
./build-examples/Release/clock_publisher_example.exe https://r2.moq.sesame-streams.com:4433 my-clock

# Room subscriber
./build-examples/Release/clock_subscriber_example.exe https://r2.moq.sesame-streams.com:4433 my-room room
```

### Example Error Handling

The examples demonstrate robust error handling:

```rust
// Operations gracefully handle network issues
match write_frame(&session, "data", payload, true).await {
    Ok(_) => println!("Data sent successfully"),
    Err(e) => {
        // This just logs a warning and continues - no need to exit!
        warn!("Failed to send data (will retry): {}", e);
    }
}
```

## Configuration

### CMake Options

- `CMAKE_BUILD_TYPE`: Build type (Debug/Release)
- `CMAKE_INSTALL_PREFIX`: Installation directory (default: /usr/local)

Note: Examples are now a separate CMake project in the `examples/` directory.

### Rust Features

The Rust library automatically builds with FFI support when building the CMake project.

## Dependencies

### Rust Dependencies
- `moq-native`: Native MOQ implementation with QUIC and MoQ networking types
- `tokio`: Async runtime
- `tracing`: Logging framework
- `anyhow`: Error handling

### C++ Dependencies
- CMake 3.16+
- C++17 compatible compiler
- Rust toolchain (for building the underlying library)

### Dependency Licensing
- `moq-cpp` ships as a compiled artifact that statically links its entire Rust
  dependency tree, so the build emits an aggregated `THIRD-PARTY-NOTICES.txt`
  reproducing every bundled crate's license text. It is generated by
  [`cargo-about`](https://github.com/EmbarkStudios/cargo-about) and installed to
  `share/moq-cpp/` alongside the library (and copied next to the built binary).
- See `THIRD_PARTY_LICENSES.md` for how attribution is generated, how to
  regenerate it, and the licenses involved.
- Most dependencies are permissively licensed (MIT or MIT OR Apache-2.0).

### Platform Dependencies

**macOS:**
- CoreFoundation framework
- Security framework

**Linux:**
- pthread
- dl (dynamic linking)

**Windows:**
- ws2_32 (Windows Sockets)
- userenv
- bcrypt

## API Documentation

### Simplified Rust API

**Core Functions:**
```rust
// Write data with automatic group management
write_frame(session: &MoqSession, track: &str, data: Vec<u8>, new_group: bool) -> Result<()>

// Write single frame in its own group (convenience method)  
write_single_frame(session: &MoqSession, track: &str, data: Vec<u8>) -> Result<()>

// Session creation (handles everything automatically)
create_publisher(url: &str, broadcast: &str, tracks: Vec<TrackDefinition>, catalog: CatalogType) -> Result<MoqSession>
create_subscriber(url: &str, broadcast: &str, tracks: Vec<TrackDefinition>, catalog: CatalogType) -> Result<MoqSession>
create_room_subscriber(url: &str, room_prefix: &str, tracks: Vec<TrackDefinition>, catalog: CatalogType) -> Result<MoqSession>
create_subscriber_with_options(url: &str, broadcast: &str, tracks: Vec<TrackDefinition>, catalog: CatalogType, subscribe_all_catalog_tracks: bool) -> Result<MoqSession>
create_room_subscriber_with_options(url: &str, room_prefix: &str, tracks: Vec<TrackDefinition>, catalog: CatalogType, subscribe_all_catalog_tracks: bool) -> Result<MoqSession>
```

**Key Benefits:**
- **Simple connection management** - sessions expose state, events, and callbacks
- **No manual track producer creation** - automatic on connection
- **Room subscriptions** - subscribe to announced broadcasts under a prefix
- **Catalog track subscriptions** - optionally subscribe to every track listed in `catalog.json`
- **Graceful error handling** - operations fail safely during network issues

Pass `subscribe_all_catalog_tracks = true` with `CatalogType::Sesame` or `CatalogType::Hang`
to subscribe to catalog-discovered tracks. Explicitly requested tracks still work; pass an
empty track list for catalog-only subscriptions.

### Simplified C++ API

**Core Methods:**
```cpp
// Write data with automatic group management  
bool WriteFrame(const std::string& track, const void* data, size_t size, bool new_group);

// Write single frame in its own group
bool WriteSingleFrame(const std::string& track, const void* data, size_t size);

// Session creation (everything automatic)
static std::unique_ptr<Session> CreatePublisher(const std::string& url, const std::string& broadcast, 
                                               const std::vector<TrackDefinition>& tracks, CatalogType catalog);
static std::unique_ptr<Session> CreateSubscriber(const std::string& url, const std::string& broadcast,
                                                 const std::vector<TrackDefinition>& tracks, CatalogType catalog,
                                                 bool subscribe_all_catalog_tracks = false);
static std::unique_ptr<Session> CreateRoomSubscriber(const std::string& url, const std::string& room_prefix,
                                                     const std::vector<TrackDefinition>& tracks, CatalogType catalog,
                                                     bool subscribe_all_catalog_tracks = false);
```

**Key Benefits:**
- **RAII resource management** - automatic cleanup
- **Thread-safe operations** - safe from multiple threads  
- **Connection callbacks** - react to disconnects and announce/unannounce events
- **Room subscriptions** - data callbacks identify the broadcast path in room mode
- **Catalog track subscriptions** - opt into all tracks from `catalog.json`

## Reconnection

A session keeps its relay connection up for as long as it lives. When the
connection drops, or the relay is not reachable when the session starts, it
reconnects with exponential backoff (`reconnect_delay` up to
`max_reconnect_delay`), forever by default (`reconnect_timeout` of zero).

- **Publishers** keep their broadcast, tracks and catalog; each new connection
  announces them again. Writes fail while `is_connected()` is false.
- **Subscribers** lose their broadcasts on a drop and subscribe again when the
  reconnected relay announces them. `broadcast_linger` keeps them announced
  across a short drop instead, at the cost of keeping a closed session's
  broadcasts alive for that long.
- **Connection closed** callbacks fire only when the connection is gone for
  good: the session gave up, or it was closed while connected.

A dead network is noticed through the QUIC idle timeout (30 s with a 5 s
keep-alive by default, see `client_config.quic`).

## Thread Safety

- **Rust**: All public APIs are Send + Sync safe - use from any async context
- **C++**: Thread-safe operations - safe to call WriteFrame from multiple threads
- **Callbacks**: May be called from background threads - ensure thread safety in your code
- **Session Management**: Connection and announce callbacks run on background threads/tasks

## Migration from Complex APIs

If you're migrating from a more complex MOQ implementation:

### What You Can Simplify
- Track producer creation and management
- Group lifecycle management
- Announcement polling and room-prefix fan-out
- Cross-language callback plumbing

### What You Keep ✅  
- Your application logic
- Data preparation and formatting
- Business logic and timing
- User interface and presentation

### Simple Migration Example

**Before (complex):**
```rust
// Old complex code
loop {
    if !session.is_connected().await {
        session.reconnect().await?;
        session.recreate_tracks().await?;
    }
    
    match session.write_data("track", data).await {
        Err(ConnectionError) => continue, // retry loop
        Err(e) => return Err(e),
        Ok(_) => break,
    }
}
```

**After (simple):**
```rust
// New simple code
if let Err(err) = write_frame(&session, "track", data, false).await {
    warn!("write failed: {}", err);
}
```

## Error Handling

### Rust Error Handling

```rust
match write_frame(&session, "track", data, true).await {
    Ok(_) => {}, // Success - data sent
    Err(e) => {
        warn!("write failed: {}", e);
    }
}
```

**Useful Signals:**
- ✅ **Connection events**: Connected and disconnected session events
- ✅ **Broadcast events**: Announced and unannounced broadcasts
- ✅ **Room data identity**: Room callbacks include `broadcast_path/track_name`
- ✅ **Write results**: Publishing methods return errors for the caller to handle

### C++ Error Handling - Simple and Safe

```cpp
// Simple boolean returns - easy to handle
if (session->WriteFrame("track", data.c_str(), data.size(), true)) {
    // Success
} else {
    std::cout << "Write failed" << std::endl;
}
```

**Callbacks Available:**
- Data callbacks
- Broadcast announced callbacks
- Broadcast cancelled callbacks
- Connection closed callbacks (the connection is gone for good)

## Contributing

1. Follow Rust conventions for Rust code
2. Follow Google C++ Style Guide for C++ code
3. Update documentation for API changes
4. Add tests for new functionality
5. Ensure examples work after changes

## License

This project is licensed under the MIT License. See `LICENSE`.
