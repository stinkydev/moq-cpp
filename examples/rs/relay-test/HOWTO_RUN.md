# Running the Relay Test Examples

This guide shows how to run both versions of the relay test application.

## Prerequisites

1. Rust toolchain installed (cargo, rustc)
2. For C++ example: CMake 3.16+ and MSVC/GCC/Clang

## Rust Examples

### Option 1: Original Version (Direct MOQ-lite)

This version directly uses the MOQ-lite API with manual track subscription.

```bash
# Build
cargo build --bin relay-test

# Run with default settings (peter broadcast on relay1)
cargo run --bin relay-test

# Run with custom settings
cargo run --bin relay-test -- --url https://relay1.moq.sesame-streams.com:4433 --broadcast peter --tracks "video/hd,audio/data"
```

**Code**: `examples/rs/relay-test/src/main.rs` (599 lines)

### Option 2: MOQ Manager Version (Simplified)

This version uses the new `moq-mgr` library with automatic catalog management.

```bash
# Build
cargo build --bin relay-test-mgr

# Run with default settings
cargo run --bin relay-test-mgr

# Run with custom settings
cargo run --bin relay-test-mgr -- --url https://relay1.moq.sesame-streams.com:4433 --broadcast peter --tracks "video/hd,audio/data"
```

**Code**: `examples/rs/relay-test/src/main_mgr.rs` (239 lines - 60% less!)

## C++ Example (MOQ Manager FFI)

This uses the C FFI layer from `moq-mgr`.

```bash
# Build (Windows with MSVC)
cmake -B build -G "Visual Studio 17 2022"
cmake --build build --config Debug

# Run
.\build\Debug\moq_relay_test_mgr.exe

# Or on Linux/macOS
cmake -B build
cmake --build build
./build/moq_relay_test_mgr
```

**Code**: `examples/cpp/relay_test_mgr.cpp`

## Keyboard Controls

All versions support the same keyboard controls:

| Key | Action |
|-----|--------|
| `c` | Connect to relay |
| `d` | Disconnect from relay |
| `s` | Show status and statistics |
| `h` | Show help |
| `q` | Quit application |

## Command Line Options

### Common Options

```
--url <URL>              MOQ relay URL (default: https://relay1.moq.sesame-streams.com:4433)
--broadcast <NAME>       Broadcast namespace to subscribe to (default: peter)
--tracks <LIST>          Comma-separated list of track names (default: video/hd,audio/data)
```

### Examples

#### Subscribe to different broadcast
```bash
cargo run --bin relay-test-mgr -- --broadcast alice
```

#### Subscribe to specific tracks
```bash
cargo run --bin relay-test-mgr -- --tracks "video/4k,audio/opus"
```

#### Use different relay
```bash
cargo run --bin relay-test-mgr -- --url https://my-relay.example.com:4433
```

## What You Should See

### On Connection
```
MOQ Relay Test Application (using MOQ Manager)
=============================================
URL: https://relay1.moq.sesame-streams.com:4433
Broadcast: peter
Tracks: video/hd, audio/data

=== Keyboard Controls ===
c - Connect to relay
d - Disconnect from relay
s - Show status
h - Show this help
q - Quit application

Note: With MOQ Manager, tracks are subscribed only when they appear in the catalog.
========================

Connecting to: https://relay1.moq.sesame-streams.com:4433
Broadcast: peter
Tracks: video/hd, audio/data
Added subscription for track: video/hd
Added subscription for track: audio/data
Session started successfully
Note: Subscriptions will activate when tracks appear in catalog
Session status: Connected
```

### During Playback
```
Track video/hd: 100 messages, 524288 bytes
Track audio/data: 100 messages, 12800 bytes
Track video/hd: 200 messages, 1048576 bytes
Track audio/data: 200 messages, 25600 bytes
...
```

### On Status Check (press 's')
```
=== Status ===
Connected: true
URL: https://relay1.moq.sesame-streams.com:4433
Broadcast: peter
Session Running: true
Track Statistics:
  - video/hd: 542 messages, 28491776 bytes
  - audio/data: 1205 messages, 154240 bytes
=============
```

## Comparing the Versions

### Feature Comparison

| Feature | Original (main.rs) | MOQ Manager (main_mgr.rs) |
|---------|-------------------|---------------------------|
| Lines of code | 599 | 239 |
| Catalog handling | Manual subscription + parsing | Automatic |
| Track subscription | Explicit subscribe calls | Callback-based |
| Data processing | Manual async loops | Callback functions |
| Reconnection | Not implemented | Built-in support |
| Error handling | Manual propagation | Callback-based |
| Complexity | High | Low |

### Performance

Both versions have similar performance characteristics:
- Same underlying MOQ-lite library
- Same QUIC transport (moq-native)
- Minimal overhead from abstraction layer
- Callback dispatch is negligible

### When to Use Which

**Use Original (main.rs)** if you need:
- Fine-grained control over subscription timing
- Custom catalog logic
- Direct access to groups and frames
- Learning the MOQ protocol details

**Use MOQ Manager (main_mgr.rs)** if you want:
- Simpler, more maintainable code
- Automatic catalog-based filtering
- Callback-based data handling
- Quick prototyping
- Focus on application logic vs protocol

## Troubleshooting

### "Failed to connect"
- Check that the relay URL is correct and accessible
- Verify you have network connectivity
- Try a different relay server

### "No data received"
- The broadcast might not be active
- Check that the broadcast name is correct
- Verify the tracks exist in the catalog (press 's' for status)

### Compilation Errors
```bash
# Clean and rebuild
cargo clean
cargo build

# Or for C++
rm -rf build
cmake -B build
cmake --build build
```

### Hanging on Startup
- This was an issue in earlier versions but has been fixed
- Make sure you're using the latest code
- The session.start() is now non-blocking

## Additional Resources

- [MOQ Manager Summary](../../docs/MOQ_MANAGER_SUMMARY.md) - Full implementation details
- [README_MGR.md](./README_MGR.md) - Detailed API comparison
- [moq-mgr API docs](../../moq-mgr/src/) - Source code with documentation

## Testing with MOQ Clock

The `moq-clock` example is a good broadcast source for testing:

```bash
# Terminal 1: Run moq-clock publisher
cd examples/rs/moq-clock
cargo run

# Terminal 2: Run relay-test-mgr subscriber
cd examples/rs/relay-test
cargo run --bin relay-test-mgr -- --broadcast clock --tracks "clock"
```

This will show real-time clock data being received!
