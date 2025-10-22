# MOQ C++ API - Clean Implementation

This is a reworked implementation of the MOQ C++ API that closely follows the concepts demonstrated in the `moq-clock` Rust example. The API provides a clean, modern C++17 interface for Media over QUIC (MOQ) applications.

## Architecture Overview

The API is organized around these key concepts:

### Core Classes

- **`Client`** - Manages connection to MOQ servers
- **`Session`** - Represents an active connection, handles broadcast publishing/consuming
- **`BroadcastProducer`** - Manages publishing multiple tracks within a broadcast
- **`BroadcastConsumer`** - Manages consuming multiple tracks from a broadcast
- **`TrackProducer`** - Publishes data to a specific track in groups
- **`TrackConsumer`** - Consumes data from a specific track in groups
- **`GroupProducer`** - Publishes frame-by-frame data within a group
- **`GroupConsumer`** - Consumes frame-by-frame data within a group

### Data Flow

```
Client -> Session -> Broadcast -> Track -> Group -> Frame
```

## Clock Example

The `clock_example.cpp` demonstrates the equivalent functionality of the Rust `moq-clock` example:

### Publisher Usage

```cpp
#include <moq/moq.h>

// Initialize and connect
auto client = moq::Client::create(config);
auto session = client->connect("https://moq.sesame-streams.com:4443");

// Create broadcast and track
auto broadcast_producer = std::make_shared<moq::BroadcastProducer>();
moq::Track track{.name = "seconds", .priority = 0};
auto track_producer = broadcast_producer->createTrack(track);

// Publish the broadcast
session->publish("clock", broadcast_producer->getConsumable());

// Send data in groups
auto group = track_producer->createGroup(sequence_number);
group->writeFrame("2024-06-24 15:30:");  // Base timestamp
group->writeFrame("45");                 // Seconds
group->finish();
```

### Subscriber Usage

```cpp
// Consume broadcast and subscribe to track
auto broadcast_consumer = session->consume("clock");
auto track_consumer = broadcast_consumer->subscribeTrack(track);

// Read groups and frames
while (auto group_future = track_consumer->nextGroup()) {
    auto group = group_future.get();
    if (!group) break;
    
    // Read base frame
    auto base_future = group->readFrame();
    auto base_data = base_future.get();
    
    // Read subsequent frames
    while (auto frame_future = group->readFrame()) {
        auto frame_data = frame_future.get();
        if (!frame_data) break;
        // Process frame data...
    }
}
```

## Key Design Principles

1. **RAII Resource Management** - All resources are automatically cleaned up
2. **Modern C++ Semantics** - Uses smart pointers, futures, and optional types
3. **Async-Ready** - Functions return futures for non-blocking operations
4. **Exception Safety** - Proper error handling without exceptions in the API
5. **Conceptual Clarity** - Maps directly to MOQ protocol concepts

## Building

```bash
mkdir build && cd build
cmake ..
ninja
```

## Running the Clock Example

### Publisher
```bash
./moq_clock_example https://moq.sesame-streams.com:4443 publish --broadcast clock --track seconds
```

### Subscriber
```bash
./moq_clock_example https://moq.sesame-streams.com:4443 subscribe --broadcast clock --track seconds
```

## Implementation Status

This is a proof-of-concept implementation that demonstrates the API design. The current implementation uses placeholder FFI calls - a full implementation would require:

1. **Extended FFI Layer** - Additional Rust functions for broadcast/track/group management
2. **Async Runtime Integration** - Proper async/await integration with Tokio
3. **Memory Management** - Correct handling of Rust-allocated objects
4. **Error Handling** - Comprehensive error propagation from Rust to C++

## Comparison with Rust moq-clock

| Rust moq-clock | C++ API |
|----------------|---------|
| `moq_lite::Session::connect()` | `Session` (from client connect) |
| `BroadcastProducer::new()` | `std::make_shared<BroadcastProducer>()` |
| `broadcast.create(track)` | `broadcast_producer->createTrack(track)` |
| `session.publish(name, broadcast)` | `session->publish(name, producer)` |
| `session.consume(&name)` | `session->consume(name)` |
| `track.create_group(seq)` | `track_producer->createGroup(seq)` |
| `group.write_frame(data)` | `group->writeFrame(data)` |
| `group.finish()` | `group->finish()` |
| `track.next_group().await` | `track_consumer->nextGroup().get()` |
| `group.read_frame().await` | `group->readFrame().get()` |

The C++ API provides the same conceptual model with C++-idiomatic patterns.

## Legacy Examples

Previous API examples that demonstrated an earlier iteration focused on direct track management rather than the broadcast/group concept hierarchy have been removed in favor of the current clock example implementation.
auto subscriber_track = subscriber_session->subscribeTrack(track_name, 
    [](const std::string& name, const std::vector<uint8_t>& data) {
        std::string received_data(data.begin(), data.end());
        std::cout << "Received: " << received_data << std::endl;
    });
```

### Setting Up Publisher
```cpp
auto publisher_track = publisher_session->publishTrack(track_name);
```

### Publishing Data
```cpp
// String data
publisher_track->sendData("Hello MOQ!");

// Binary data  
std::vector<uint8_t> binary_data = {0x48, 0x65, 0x6c, 0x6c, 0x6f};
publisher_track->sendData(binary_data);

// Raw pointer data
const uint8_t* raw_data = ...;
size_t data_size = ...;
publisher_track->sendData(raw_data, data_size);
```

## Building and Running

```bash
# Build the project
cd build && ninja

# Run the clock example
./moq_clock_example https://moq.sesame-streams.com:4443 publish --broadcast clock
./moq_clock_example https://moq.sesame-streams.com:4443 subscribe --broadcast clock

# Run the relay test (direct MOQ API)
./moq_relay_test --url https://relay2.moq.sesame-streams.com:4433 --broadcast peter --tracks video,audio

# Run the relay test with MOQ Manager (simplified abstraction)
./moq_relay_test_mgr --url https://relay2.moq.sesame-streams.com:4433 --broadcast peter --tracks video,audio
```

## Examples

### relay_test.cpp vs relay_test_mgr.cpp

Two equivalent examples are provided to demonstrate different API approaches:

**`relay_test.cpp`** - Uses the direct MOQ API with manual session/broadcast/track management:
- Manual connection lifecycle management
- Explicit track subscription/unsubscription controls
- Direct handling of MOQ groups and frames
- More control but requires more code

**`relay_test_mgr.cpp`** - Uses the MOQ Manager abstraction for simplified session management:
- Automatic session lifecycle management with reconnection
- All configured tracks are subscribed automatically when connecting
- Simplified data handling through callbacks
- Less control but much simpler code

The MOQ Manager approach is recommended for applications that don't need fine-grained control over individual track subscriptions and prefer automatic session management.

## Architecture Notes

- **Memory Safety**: All resources are managed through RAII principles
- **Type Safety**: Strong typing with proper C++ abstractions over C FFI
- **Error Handling**: Comprehensive error checking at both FFI and C++ levels
- **Callback Management**: Safe callback storage and execution
- **Thread Safety**: Proper synchronization for global state management

This implementation provides a solid foundation for MOQ applications with clean, modern C++ interfaces over the Rust MOQ native library.
