hd,# MOQ Relay Test - MOQ Manager Version

This directory contains two versions of the relay test application:

## Version Comparison

### `main.rs` - Direct MOQ-lite Usage
- **Lines of code**: 599 lines
- **Approach**: Manual track subscription using `TrackSubscriber`
- **Features**:
  - Direct control over track subscription
  - Manual `next_group()` and frame reading loops
  - Explicit async/await management
  - Full control but more complex code

### `main_mgr.rs` - MOQ Manager Usage  
- **Lines of code**: 239 lines (60% reduction!)
- **Approach**: High-level `Session` API with automatic management
- **Features**:
  - Simplified session management with `Session::new()`
  - Automatic catalog-based subscription
  - Callback-based data handling
  - Built-in reconnection support
  - Much simpler and cleaner code

## Key Differences

### Connection Setup

**Direct (main.rs)**:
```rust
let client = moq_native::Client::new(client_config)?;
let session = client.connect(&url).await?;
let subscriber = TrackSubscriber::new(session.clone(), &config).await?;
// Manual loop management...
```

**MOQ Manager (main_mgr.rs)**:
```rust
let session = Session::new(session_config, SessionMode::SubscribeOnly);
session.set_status_callback(|status| { /* ... */ });
session.start().await?;
```

### Track Subscription

**Direct (main.rs)**:
```rust
let track = session.subscribe(&namespace, &track_name).await?;
let mut consumer = TrackConsumer::new(track);
loop {
    let mut group = consumer.next_group(None).await?;
    // Manual frame reading...
}
```

**MOQ Manager (main_mgr.rs)**:
```rust
session.add_subscription(SubscriptionConfig {
    moq_track_name: "video/hd".to_string(),
    data_callback: Arc::new(|data: &[u8]| {
        println!("Received {} bytes", data.len());
    }),
});
```

### Catalog Handling

**Direct (main.rs)**:
- Must manually subscribe to `catalog.json` track
- Parse JSON manually
- Track availability yourself
- Subscribe to tracks explicitly

**MOQ Manager (main_mgr.rs)**:
- Catalog is automatically subscribed internally
- Tracks only activate when they appear in catalog
- No manual catalog parsing needed
- Implements the requirement: "Tracks that user wants to subscribe to should not be subscribed unless they exist in catalog track"

## Running the Examples

### Run Original Version
```bash
cargo run --bin relay-test -- --url https://relay.moq.sesame-streams.com:4433 --broadcast peter
```

### Run MOQ Manager Version
To use the MOQ Manager version, you need to modify `Cargo.toml` to set the binary:

```toml
[[bin]]
name = "relay-test-mgr"
path = "src/main_mgr.rs"
```

Then run:
```bash
cargo run --bin relay-test-mgr -- --url https://relay.moq.sesame-streams.com:4433 --broadcast peter
```

## Keyboard Controls

Both versions support:
- `c` - Connect to relay
- `d` - Disconnect from relay
- `s` - Show status and statistics
- `h` - Show help
- `q` - Quit application

## Benefits of MOQ Manager

1. **Simpler Code**: ~60% less code for the same functionality
2. **Automatic Catalog Management**: No need to manually track catalog state
3. **Safer Subscription**: Tracks only subscribe when available in catalog
4. **Reconnection Support**: Built-in automatic reconnection on failure
5. **Callback-Based**: No manual async loops needed for data processing
6. **Better Separation**: Business logic separated from MOQ protocol details

## When to Use Each

**Use Direct API (main.rs)** when:
- You need fine-grained control over subscription timing
- You want to implement custom catalog logic
- You're building a low-level MOQ tool
- You need to handle groups/frames specially

**Use MOQ Manager (main_mgr.rs)** when:
- You want simple publish/subscribe functionality
- You want catalog-based subscription filtering
- You need automatic reconnection
- You prefer callback-based APIs
- You want to focus on application logic, not MOQ protocol

## Architecture

The MOQ Manager (`moq-mgr` crate) provides:
- **Session**: Connection and lifecycle management
- **CatalogProcessor**: Automatic catalog.json handling  
- **Producer**: Simple broadcast API
- **Consumer**: Callback-based subscription API
- **FFI Layer**: C++ integration via cbindgen

This creates a clean abstraction over the lower-level `moq-lite` and `moq-native` APIs.
