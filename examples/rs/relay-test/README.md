# MOQ Relay Test (Rust Native)

This is a Rust version of the relay test application that communicates directly with the native Rust MOQ API instead of going through the C++ wrapper and FFI layer.

## Features

- Interactive keyboard-driven interface
- Connect/disconnect to MOQ relay servers
- Subscribe/unsubscribe to individual tracks (video, audio)
- Real-time statistics showing bytes received per track
- Async handling of multiple track subscriptions
- Graceful shutdown and cleanup

## Usage

```bash
# Run with default settings (connects to relay2.moq.sesame-streams.com:4433, broadcast "peter")
cargo run --bin relay-test

# Specify custom relay and broadcast
cargo run --bin relay-test -- --url https://relay.example.com:4433 --broadcast mybcast

# Specify custom tracks
cargo run --bin relay-test -- --tracks video,audio,metadata

# Show help
cargo run --bin relay-test -- --help
```

## Keyboard Controls

Once the application is running, use these keyboard shortcuts:

- `c` - Connect to relay
- `d` - Disconnect from relay  
- `v` - Subscribe to video track
- `a` - Subscribe to audio track
- `V` - Unsubscribe from video track
- `A` - Unsubscribe from audio track
- `u` - Unsubscribe from all tracks
- `s` - Show status and statistics
- `h` - Show help
- `q` - Quit application

## Architecture

This application demonstrates the native Rust MOQ API usage:

1. **moq_native::Client** - Creates client connection to relay
2. **moq_lite::Session** - Manages the session with subscribe-only mode
3. **moq_lite::Origin** - Handles broadcast consumption
4. **BroadcastConsumer** - Subscribes to individual tracks
5. **TrackConsumer** - Receives groups and frames for a specific track
6. **GroupConsumer** - Reads frame data from groups

The application uses async/await with Tokio for handling multiple concurrent track subscriptions and keyboard input.

## Differences from C++ Version

- Uses native Rust async/await instead of futures with timeouts
- Direct API access without FFI overhead
- Uses `crossterm` for cross-platform keyboard input instead of Windows-specific `_kbhit()`
- Leverages Rust's ownership system for automatic cleanup
- Uses `tokio::select!` for cancellation instead of atomic flags

## Dependencies

- `moq-lite` - Core MOQ protocol implementation
- `moq-native` - Native transport layer (Quinn/QUIC)
- `tokio` - Async runtime
- `crossterm` - Cross-platform terminal handling
- `clap` - Command-line argument parsing
- `anyhow` - Error handling
- `url` - URL parsing