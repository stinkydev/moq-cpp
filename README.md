# MOQ Wrapper Project

A comprehensive wrapper library for MOQ (Media over QUIC) with automatic reconnection and simplified APIs for both Rust and C++.

## Project Structure

```
/
├── src/                    # Rust source code
│   ├── lib.rs             # Simplified public API
│   ├── session.rs         # Session management with auto-reconnection
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
use moq_wrapper::{init, create_publisher, create_subscriber, write_frame, TrackDefinition, Level};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize with logging
    init(Level::INFO, None);
    
    // Create publisher with track definitions
    let tracks = vec![
        TrackDefinition::data("seconds", 0), // Simple data track
    ];
    
    let session = create_publisher(
        "https://relay1.moq.sesame-streams.com:4433",
        "my-broadcast",
        tracks,
        CatalogType::None  // No catalog needed for simple use cases
    ).await?;
    
    // No manual connection waiting needed - session handles everything automatically
    
    // Write data with automatic group management
    write_frame(&session, "seconds", "2024-11-05 12:00:00".into(), true).await?; // new_group=true
    write_frame(&session, "seconds", "30".into(), false).await?; // add to current group
    
    // All errors are handled gracefully - no need for manual reconnection logic
    
    Ok(())
}
```

### C++ API - Clean Interface

```cpp
#include "moq_wrapper.h"
#include <iostream>

int main() {
    // Initialize with logging
    moq::Init(moq::LogLevel::kInfo);
    
    // Create publisher with simple track definition
    std::vector<moq::TrackDefinition> tracks;
    tracks.emplace_back("seconds", 0, moq::TrackType::kData);
    
    auto session = moq::Session::CreatePublisher(
        "https://relay1.moq.sesame-streams.com:4433",
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
- **Automatic Reconnection**: Infinite reconnection attempts with exponential backoff - never gives up
- **Simplified API**: Just two main functions - `write_frame()` and `write_single_frame()`
- **Bulletproof Error Handling**: All operations handle network interruptions gracefully
- **Zero Manual Connection Management**: No need to check connection status or implement reconnection logic
- **Track Auto-Creation**: Track producers are automatically created and managed
- **Session-Level Resilience**: Applications continue running even during network outages

### Rust Features  
- **Async/Await**: Full async support with Tokio runtime
- **Graceful Degradation**: Operations fail gracefully during reconnection, resume automatically
- **Clean API**: `write_frame(session, track, data, new_group)` - that's it!
- **Type Safety**: Strong typing for all MOQ concepts
- **Production Ready**: Used in real-world applications with network instability

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
- ✅ **State synchronization after reconnection**

## Examples

### Running Rust Examples

```bash
# Clock publisher (with automatic reconnection)
cargo run --example clock_example -- \
  --url https://relay1.moq.sesame-streams.com:4433 \
  --broadcast my-clock \
  publish

# Clock subscriber (resilient to publisher disconnections)
cargo run --example clock_example -- \
  --url https://relay1.moq.sesame-streams.com:4433 \
  --broadcast my-clock \
  subscribe

# Hang subscriber  
cargo run --example hang_subscriber \
  --url https://relay1.moq.sesame-streams.com:4433 \
  --broadcast hang-broadcast
```

### Running C++ Examples

```bash
# Build examples first
cmake .. -DBUILD_EXAMPLES=ON
cmake --build .

# Clock publisher (automatically handles reconnection)
./clock_publisher_cpp https://relay1.moq.sesame-streams.com:4433 my-clock

# Clock subscriber (resilient to network issues)
./clock_subscriber_cpp https://relay1.moq.sesame-streams.com:4433 my-clock
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
- `moq-lite`: Core MOQ protocol implementation
- `moq-native`: Native MOQ implementation with QUIC
- `tokio`: Async runtime
- `tracing`: Logging framework
- `anyhow`: Error handling

### C++ Dependencies
- CMake 3.16+
- C++17 compatible compiler
- Rust toolchain (for building the underlying library)

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
```

**Key Benefits:**
- **No connection management needed** - sessions handle everything
- **No manual track producer creation** - automatic on connection
- **No reconnection logic required** - infinite retry built-in
- **Graceful error handling** - operations fail safely during network issues

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
```

**Key Benefits:**
- **RAII resource management** - automatic cleanup
- **Thread-safe operations** - safe from multiple threads  
- **No manual connection checking** - just call methods
- **Automatic reconnection** - transparent to application

## Thread Safety

- **Rust**: All public APIs are Send + Sync safe - use from any async context
- **C++**: Thread-safe operations - safe to call WriteFrame from multiple threads
- **Callbacks**: May be called from background threads - ensure thread safety in your code
- **Session Management**: All reconnection logic runs on background tasks - non-blocking

## Migration from Complex APIs

If you're migrating from a more complex MOQ implementation:

### What You Can Remove ❌
- Manual connection status checking
- Reconnection logic and retry loops  
- Track producer creation and management
- Group lifecycle management
- Network error recovery code
- Connection state synchronization

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
write_frame(&session, "track", data, false).await?;
// That's it! All error handling and reconnection is automatic
```

## Error Handling

### Rust Error Handling - Bulletproof by Design

```rust
// All operations handle errors gracefully - no manual error checking needed!
match write_frame(&session, "track", data, true).await {
    Ok(_) => {}, // Success - data sent
    Err(e) => {
        // Just log and continue - session will reconnect automatically
        warn!("Temporary failure: {}", e);
        // Application keeps running, will work again when connection restored
    }
}
```

**Built-in Error Recovery:**
- ✅ **Network disconnections**: Infinite reconnection attempts
- ✅ **Track producer failures**: Automatic recreation
- ✅ **Group management errors**: Automatic cleanup and retry
- ✅ **Session failures**: Transparent reconnection
- ✅ **Relay unavailability**: Keeps trying until available

### C++ Error Handling - Simple and Safe

```cpp
// Simple boolean returns - easy to handle
if (session->WriteFrame("track", data.c_str(), data.size(), true)) {
    // Success
} else {
    // Temporary failure - session will reconnect automatically
    std::cout << "Temporary failure, will retry automatically" << std::endl;
}
```

**No Manual Recovery Needed:**
- Session handles all reconnection logic internally
- Applications continue running during network outages  
- Operations resume automatically when connection restored

## Contributing

1. Follow Rust conventions for Rust code
2. Follow Google C++ Style Guide for C++ code
3. Update documentation for API changes
4. Add tests for new functionality
5. Ensure examples work after changes

## License

This project is dual-licensed under MIT OR Apache-2.0.