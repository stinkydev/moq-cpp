# MOQ C++ API

A modern C++ wrapper around the [moq-native](https://github.com/kixelated/moq-rs) Rust library for **Media over QUIC (MOQ)** applications. This library provides a clean, type-safe C++ interface for real-time media streaming using the MOQ protocol.

## What is MOQ?

Media over QUIC (MOQ) is a live media delivery protocol that leverages QUIC's advantages:
- **Low latency**: Stream media with minimal delay
- **Connection multiplexing**: Multiple streams over single connection  
- **Built-in reliability**: Automatic retransmission and congestion control
- **Modern security**: TLS 1.3 encryption by default

## Key Features

- 🚀 **Modern C++17** interface with RAII and smart pointers
- 🔧 **Cross-platform**: Windows, macOS, and Linux support
- 📦 **Easy integration** via CMake
- 🛡️ **Memory safe** Rust backend with C++ convenience
- 🔄 **Producer/Consumer** pattern for streaming data
- 📊 **Hierarchical data** organization (Broadcasts → Tracks → Groups → Frames)

## Quick Start

```cpp
#include <moq/moq.h>

int main() {
    // Initialize the library
    moq::Client::initialize();
    
    // Create and configure client
    moq::ClientConfig config;
    config.bind_addr = "0.0.0.0:0";  // IPv4
    config.tls_disable_verify = true; // For testing
    
    auto client = moq::Client::create(config);
    auto session = client->connect("https://relay.moq.example.com:4433");
    
    if (session) {
        std::cout << "Connected to MOQ relay!" << std::endl;
        // Now you can publish or subscribe to streams
    }
    
    return 0;
}

## Overview

This project provides:
- **Rust FFI layer** (`moq-ffi`): C-compatible interface around moq-native
- **C++ wrapper** (`cpp/`): C++ classes and RAII semantics
- **CMake build system**: Easy integration into C++ projects
- **Examples**: Sample code demonstrating usage

## MOQ Concepts

### Data Hierarchy

MOQ organizes data in a hierarchical structure:

```
Broadcast
  └── Track (named stream within broadcast)
      └── Group (sequence of related frames)
          └── Frame (individual data packet)
```

**Broadcast**: A collection of related tracks (e.g., "clock" broadcast)
**Track**: A named stream within a broadcast (e.g., "seconds" track)  
**Group**: A sequence number-ordered collection of frames (e.g., data for one minute)
**Frame**: Individual data packets within a group (e.g., each second update)

### Producer/Consumer Pattern

- **Producer**: Publishes data to tracks using `BroadcastProducer` → `TrackProducer` → `GroupProducer`
- **Consumer**: Subscribes to tracks using `BroadcastConsumer` → `TrackConsumer` → `GroupConsumer`

### Sequence Ordering

- **Groups** are identified by sequence numbers for ordered delivery
- **Frames** within a group maintain their order
- Consumers can process groups as they arrive or wait for specific sequence numbers

## Threading Strategy

### Overview

The MOQ C++ API provides a **blocking interface** over an **asynchronous Rust backend**. Understanding this design is crucial for proper usage:

- **Rust Layer**: Uses Tokio async runtime for all network operations
- **C++ Layer**: Provides synchronous, blocking methods that internally await async operations
- **Thread Safety**: The API is **not thread-safe** - external synchronization is required

### Key Threading Principles

#### 1. **One Operation Per Object**
```cpp
// ❌ BAD: Concurrent operations on same object
auto group = track->nextGroup();  // Thread 1
auto frame = group->readFrame();  // Thread 2 - UNSAFE!

// ✅ GOOD: Sequential operations
auto group = track->nextGroup();
auto frame = group->readFrame();
```

#### 2. **Blocking Operations**
All MOQ operations are **blocking** and will wait for network I/O:
```cpp
// These calls block until data is available or connection fails
auto group = trackConsumer->nextGroup();      // Waits for next group
auto frame = groupConsumer->readFrame();      // Waits for next frame
auto session = client->connect(url);          // Waits for connection
```

#### 3. **No Internal Threading**
The C++ wrapper does **not** create background threads. All async work happens in the Rust Tokio runtime, but C++ calls are synchronous.

### Recommended Patterns

#### Pattern 1: **Single-Threaded Consumer**
```cpp
void consumeTrack(std::unique_ptr<moq::TrackConsumer> track) {
    while (true) {
        auto group = track->nextGroup();  // Blocks until available
        if (!group) break;  // Connection closed
        
        while (true) {
            auto frame = group->readFrame();  // Blocks until available
            if (!frame) break;  // No more frames in group
            
            processFrame(*frame);  // Your processing logic
        }
    }
}
```

#### Pattern 2: **Multi-Threaded with External Synchronization**
```cpp
class ThreadSafeConsumer {
private:
    std::unique_ptr<moq::TrackConsumer> track_;
    std::mutex track_mutex_;
    std::queue<std::vector<uint8_t>> frame_queue_;
    std::mutex queue_mutex_;
    std::condition_variable cv_;
    std::atomic<bool> stop_{false};

public:
    void startConsumerThread() {
        std::thread([this]() {
            while (!stop_) {
                std::unique_ptr<moq::GroupConsumer> group;
                {
                    std::lock_guard<std::mutex> lock(track_mutex_);
                    group = track_->nextGroup();  // Blocks
                }
                
                if (!group) break;
                
                while (!stop_) {
                    auto frame = group->readFrame();  // Blocks
                    if (!frame) break;
                    
                    // Add to thread-safe queue
                    {
                        std::lock_guard<std::mutex> lock(queue_mutex_);
                        frame_queue_.push(std::move(*frame));
                    }
                    cv_.notify_one();
                }
            }
        }).detach();
    }
    
    std::optional<std::vector<uint8_t>> getFrame() {
        std::unique_lock<std::mutex> lock(queue_mutex_);
        cv_.wait(lock, [this]() { return !frame_queue_.empty() || stop_; });
        
        if (frame_queue_.empty()) return std::nullopt;
        
        auto frame = std::move(frame_queue_.front());
        frame_queue_.pop();
        return frame;
    }
};
```

#### Pattern 3: **Producer with Background Publishing**
```cpp
class AsyncProducer {
private:
    std::unique_ptr<moq::TrackProducer> track_;
    std::queue<std::vector<uint8_t>> send_queue_;
    std::mutex queue_mutex_;
    std::condition_variable cv_;
    std::atomic<bool> stop_{false};

public:
    void startPublisherThread() {
        std::thread([this]() {
            uint64_t sequence = 0;
            
            while (!stop_) {
                auto group = track_->createGroup(sequence++);
                
                // Send all queued data in this group
                while (!stop_) {
                    std::vector<uint8_t> data;
                    {
                        std::unique_lock<std::mutex> lock(queue_mutex_);
                        cv_.wait_for(lock, std::chrono::milliseconds(100), 
                                   [this]() { return !send_queue_.empty() || stop_; });
                        
                        if (send_queue_.empty()) break;  // No data, end group
                        
                        data = std::move(send_queue_.front());
                        send_queue_.pop();
                    }
                    
                    if (!group->writeFrame(data)) break;  // Write failed
                }
                
                group->finish();  // Complete the group
            }
        }).detach();
    }
    
    void queueFrame(const std::vector<uint8_t>& data) {
        std::lock_guard<std::mutex> lock(queue_mutex_);
        send_queue_.push(data);
        cv_.notify_one();
    }
};
```

### Threading Best Practices

#### ✅ **DO:**
- Use external synchronization (mutexes, atomics) for multi-threading
- Handle blocking operations appropriately (they may wait indefinitely)
- Check return values - `nullptr`/`nullopt` indicates connection closed or error
- Use separate threads for producers and consumers when needed
- Design your threading model around the blocking nature of operations

#### ❌ **DON'T:**
- Call methods on the same object from multiple threads simultaneously
- Assume operations are non-blocking or have timeouts
- Rely on the API for internal thread management
- Mix sync/async patterns - stick to the blocking model

### Error Handling in Multi-Threaded Context

```cpp
void robustConsumer() {
    try {
        while (true) {
            auto group = track_->nextGroup();
            if (!group) {
                // Connection closed gracefully
                break;
            }
            
            while (true) {
                auto frame = group->readFrame();
                if (!frame) {
                    // End of group, not an error
                    break;
                }
                
                if (!processFrame(*frame)) {
                    // Processing error, decide whether to continue
                    continue;  // or break, depending on your needs
                }
            }
        }
    } catch (const std::exception& e) {
        // Handle connection errors
        std::cerr << "Consumer error: " << e.what() << std::endl;
        // Implement reconnection logic if needed
    }
}
```

### Performance Considerations

- **Blocking Calls**: Operations block until network I/O completes
- **No Timeouts**: Operations may wait indefinitely - implement application-level timeouts if needed
- **Memory Management**: Objects use RAII - no manual cleanup required
- **Network Buffering**: The Rust backend handles buffering and flow control

## Architecture

```
┌─────────────────────┐
│   C++ Application   │
├─────────────────────┤
│   C++ Wrapper       │  <- moq::Client, moq::Session classes
├─────────────────────┤
│   C FFI Layer       │  <- moq_client_new, moq_client_connect, etc.
├─────────────────────┤
│   Rust moq-native   │  <- Original moq-native library
└─────────────────────┘
```

## Prerequisites

- **Rust**: Install from [rustup.rs](https://rustup.rs/)
- **CMake**: Version 3.16 or later
- **C++ Compiler**: Supporting C++17 or later
- **Git**: For cloning dependencies

### Platform-specific requirements

#### macOS
```bash
# Install Xcode command line tools
xcode-select --install
```

#### Linux (Ubuntu/Debian)
```bash
sudo apt update
sudo apt install build-essential cmake git curl
```

#### Windows
- Visual Studio 2019 or later with C++ support
- Or MinGW-w64 with MSYS2

## Building

#### Continuous Integration

The project includes a CI/CD pipeline using GitHub Actions:

- **Multi-platform builds**: Tests on Ubuntu, macOS, and Windows
- **Rust toolchain**: Automatic installation and caching
- **Code quality**: Formatting checks, linting with Clippy
- **Build verification**: Compiles and runs the example application

The CI pipeline automatically:
1. Installs dependencies (Rust, CMake, Ninja)
2. Caches Rust dependencies for faster builds
3. Builds the project in release mode
4. Runs the example to verify functionality
5. Checks Rust code formatting and linting

## Quick Start

```bash
# Clone the repository
git clone <your-repo-url> moq-c-api
cd moq-c-api

# Create build directory
mkdir build && cd build

# Configure and build
cmake ..
make -j$(nproc)

# Run the example
./moq_example
```

### Detailed Build Process

1. **Configure the build**:
   ```bash
   cmake -B build -DCMAKE_BUILD_TYPE=Release
   ```

2. **Build the project**:
   ```bash
   cmake --build build --parallel
   ```

3. **Install (optional)**:
   ```bash
   cmake --install build --prefix /usr/local
   ```

## Complete Examples

### Clock Publisher (Real-time Timestamp Streaming)

```cpp
#include <moq/moq.h>
#include <iostream>
#include <chrono>
#include <thread>

class ClockPublisher {
private:
    std::unique_ptr<moq::TrackProducer> track_;

public:
    ClockPublisher(std::unique_ptr<moq::TrackProducer> track) 
        : track_(std::move(track)) {}

    void run() {
        uint64_t sequence = 0;
        
        while (true) {
            // Create group for this timestamp
            auto group = track_->createGroup(sequence++);
            
            // Get current time
            auto now = std::chrono::system_clock::now();
            auto time_t = std::chrono::system_clock::to_time_t(now);
            std::string timestamp = std::ctime(&time_t);
            
            // Send timestamp
            group->writeFrame(timestamp);
            group->finish();
            
            std::cout << "Published: " << timestamp;
            std::this_thread::sleep_for(std::chrono::seconds(1));
        }
    }
};

int main() {
    // Initialize and connect
    moq::Client::initialize();
    
    moq::ClientConfig config;
    config.bind_addr = "0.0.0.0:0";
    config.tls_disable_verify = true;
    
    auto client = moq::Client::create(config);
    auto session = client->connect("https://relay.moq.example.com:4433");
    
    if (!session) {
        std::cerr << "Failed to connect" << std::endl;
        return 1;
    }
    
    // Set up publishing
    moq::Track track = {"seconds", 0};
    auto broadcast = std::make_shared<moq::BroadcastProducer>();
    auto track_producer = broadcast->createTrack(track);
    
    session->publish("clock", broadcast->getConsumable());
    
    // Start publishing
    ClockPublisher publisher(std::move(track_producer));
    publisher.run();
    
    return 0;
}
```

### Clock Subscriber (Receiving Timestamps)

```cpp
#include <moq/moq.h>
#include <iostream>

class ClockSubscriber {
private:
    std::unique_ptr<moq::TrackConsumer> track_;

public:
    ClockSubscriber(std::unique_ptr<moq::TrackConsumer> track) 
        : track_(std::move(track)) {}

    void run() {
        while (true) {
            // Wait for next group
            auto group_future = track_->nextGroup();
            auto group = group_future.get();
            
            if (!group) {
                std::cout << "Stream ended" << std::endl;
                break;
            }
            
            // Read all frames from group
            while (true) {
                auto frame_future = group->readFrame();
                auto frame_data = frame_future.get();
                
                if (!frame_data) break; // No more frames
                
                std::string timestamp(frame_data->begin(), frame_data->end());
                std::cout << "Received: " << timestamp;
            }
        }
    }
};

int main() {
    // Initialize and connect  
    moq::Client::initialize();
    
    moq::ClientConfig config;
    config.bind_addr = "0.0.0.0:0";
    config.tls_disable_verify = true;
    
    auto client = moq::Client::create(config);
    auto session = client->connect("https://relay.moq.example.com:4433");
    
    if (!session) {
        std::cerr << "Failed to connect" << std::endl;
        return 1;
    }
    
    // Set up subscription
    moq::Track track = {"seconds", 0};
    auto broadcast_consumer = session->consume("clock");
    auto track_consumer = broadcast_consumer->subscribeTrack(track);
    
    // Start consuming
    ClockSubscriber subscriber(std::move(track_consumer));
    subscriber.run();
    
    return 0;
}
```

### Multi-Track Broadcast (Video + Audio + Metadata)

```cpp
#include <moq/moq.h>
#include <iostream>
#include <thread>

class MultiTrackPublisher {
private:
    std::unique_ptr<moq::TrackProducer> video_track_;
    std::unique_ptr<moq::TrackProducer> audio_track_;
    std::unique_ptr<moq::TrackProducer> metadata_track_;

public:
    MultiTrackPublisher(std::unique_ptr<moq::TrackProducer> video,
                       std::unique_ptr<moq::TrackProducer> audio,
                       std::unique_ptr<moq::TrackProducer> metadata)
        : video_track_(std::move(video))
        , audio_track_(std::move(audio)) 
        , metadata_track_(std::move(metadata)) {}

    void run() {
        uint64_t sequence = 0;
        
        while (true) {
            // Publish video frame
            auto video_group = video_track_->createGroup(sequence);
            video_group->writeFrame("video_frame_data_" + std::to_string(sequence));
            video_group->finish();
            
            // Publish audio frame  
            auto audio_group = audio_track_->createGroup(sequence);
            audio_group->writeFrame("audio_sample_data_" + std::to_string(sequence));
            audio_group->finish();
            
            // Publish metadata (every 10th frame)
            if (sequence % 10 == 0) {
                auto meta_group = metadata_track_->createGroup(sequence / 10);
                meta_group->writeFrame("timestamp=" + std::to_string(sequence));
                meta_group->writeFrame("resolution=1920x1080");
                meta_group->finish();
            }
            
            std::cout << "Published frame " << sequence << std::endl;
            sequence++;
            
            std::this_thread::sleep_for(std::chrono::milliseconds(33)); // ~30 FPS
        }
    }
};

int main() {
    moq::Client::initialize();
    
    moq::ClientConfig config;
    config.bind_addr = "0.0.0.0:0";
    config.tls_disable_verify = true;
    
    auto client = moq::Client::create(config);
    auto session = client->connect("https://relay.moq.example.com:4433");
    
    // Create tracks with different priorities
    moq::Track video_track = {"video", 0};      // Highest priority
    moq::Track audio_track = {"audio", 1};      // Medium priority  
    moq::Track metadata_track = {"metadata", 2}; // Lowest priority
    
    auto broadcast = std::make_shared<moq::BroadcastProducer>();
    auto video_producer = broadcast->createTrack(video_track);
    auto audio_producer = broadcast->createTrack(audio_track);
    auto metadata_producer = broadcast->createTrack(metadata_track);
    
    session->publish("live-stream", broadcast->getConsumable());
    
    MultiTrackPublisher publisher(std::move(video_producer),
                                 std::move(audio_producer), 
                                 std::move(metadata_producer));
    publisher.run();
    
    return 0;
}
```

## Threading Quick Reference

### ⚡ **Key Points**
- **All operations are BLOCKING** - they wait for network I/O
- **NOT thread-safe** - use external synchronization for multi-threading
- **No timeouts** - operations may wait indefinitely
- **RAII cleanup** - objects auto-cleanup when destroyed

### 📋 **Threading Checklist**

**Before implementing multi-threading:**
- [ ] Do I need multiple threads? (Single-threaded is often simpler)
- [ ] How will I synchronize access to MOQ objects?
- [ ] How will I handle blocking operations?
- [ ] How will I detect and handle connection failures?

**For each MOQ object:**
- [ ] Only one thread calls methods at a time
- [ ] Proper mutex protection if shared between threads
- [ ] Error handling for connection failures
- [ ] Graceful shutdown mechanism

### 🔄 **Common Patterns**

| Pattern | Use Case | Complexity |
|---------|----------|------------|
| **Single-threaded** | Simple consumers/producers | ⭐ Low |
| **Producer + Consumer threads** | Separate read/write logic | ⭐⭐ Medium |
| **Thread pool** | Multiple parallel streams | ⭐⭐⭐ High |
| **Async wrapper** | Non-blocking interface | ⭐⭐⭐⭐ Very High |

### 🚨 **Common Mistakes**
```cpp
// ❌ Multiple threads, same object
std::thread t1([&]() { consumer->nextGroup(); });
std::thread t2([&]() { consumer->nextGroup(); });  // RACE CONDITION!

// ❌ No error handling
auto frame = group->readFrame();  // May return nullopt!
processFrame(*frame);  // CRASH if connection closed!

// ✅ Correct approach
std::mutex consumer_mutex;
std::thread t1([&]() {
    std::lock_guard<std::mutex> lock(consumer_mutex);
    auto group = consumer->nextGroup();
    if (group) {
        auto frame = group->readFrame();
        if (frame) {
            processFrame(*frame);
        }
    }
});
```

## API Reference

### Core Classes

#### `moq::Client`
Main entry point for MOQ connections. Manages the underlying QUIC client.

```cpp
class Client {
public:
    // Static initialization (call once per process)
    static Result initialize();
    
    // Factory method to create client instances
    static std::unique_ptr<Client> create(const ClientConfig& config);
    
    // Connect to a MOQ relay server
    std::unique_ptr<Session> connect(const std::string& url);
    
    // Get detailed error information
    std::string getLastError() const;
    
    // Convert result codes to human-readable strings
    static std::string resultToString(Result result);
};
```

**Usage Example:**
```cpp
// Initialize once per process
auto result = moq::Client::initialize();
if (result != moq::Result::Success) {
    std::cerr << "Init failed: " << moq::Client::resultToString(result) << std::endl;
}

// Create client instance
moq::ClientConfig config;
config.bind_addr = "0.0.0.0:0";
config.tls_disable_verify = true;

auto client = moq::Client::create(config);
auto session = client->connect("https://relay.example.com:4433");
```

#### `moq::Session`
Represents an active connection to a MOQ relay server. Used for publishing and subscribing to broadcasts.

```cpp
class Session {
public:
    // Check connection status
    bool isConnected() const;
    
    // Gracefully close the connection
    void close();
    
    // Publish a broadcast (for producers)
    bool publish(const std::string& broadcast_name, 
                std::shared_ptr<BroadcastConsumer> consumable);
    
    // Subscribe to a broadcast (for consumers)
    std::unique_ptr<BroadcastConsumer> consume(const std::string& broadcast_name);
};
```

#### `moq::ClientConfig`
Configuration options for MOQ clients.

```cpp
struct ClientConfig {
    std::string bind_addr = "[::]:0";           // Local bind address
    bool tls_disable_verify = false;            // Disable TLS verification
    std::string tls_root_cert_path = "";        // Custom root certificate
};
```

**Configuration Examples:**
```cpp
// IPv4 configuration
moq::ClientConfig config_v4;
config_v4.bind_addr = "0.0.0.0:0";

// IPv6 configuration  
moq::ClientConfig config_v6;
config_v6.bind_addr = "[::]:0";

// Custom certificate
moq::ClientConfig config_cert;
config_cert.tls_root_cert_path = "/path/to/ca-cert.pem";
```

### Broadcasting API

#### `moq::BroadcastProducer`
Manages the publication of multiple tracks within a single broadcast namespace.

```cpp
class BroadcastProducer {
public:
    BroadcastProducer();
    ~BroadcastProducer();
    
    // Create a track producer for publishing data
    std::unique_ptr<TrackProducer> createTrack(const Track& track);
    
    // Get consumable interface for session publishing
    std::shared_ptr<BroadcastConsumer> getConsumable();
};
```

#### `moq::BroadcastConsumer`  
Manages subscription to multiple tracks within a broadcast.

```cpp
class BroadcastConsumer {
public:
    ~BroadcastConsumer();
    
    // Subscribe to a specific track within the broadcast
    std::unique_ptr<TrackConsumer> subscribeTrack(const Track& track);
};
```

**Broadcasting Example:**
```cpp
// Producer side
auto broadcast = std::make_shared<moq::BroadcastProducer>();
auto track_producer = broadcast->createTrack({"video", 0});
session->publish("my-stream", broadcast->getConsumable());

// Consumer side  
auto broadcast_consumer = session->consume("my-stream");
auto track_consumer = broadcast_consumer->subscribeTrack({"video", 0});
```

### Track API

#### `moq::Track`
Metadata describing a track within a broadcast.

```cpp
struct Track {
    std::string name;    // Track identifier (e.g., "video", "audio", "metadata")
    uint8_t priority;    // Delivery priority (0 = highest priority)
};
```

#### `moq::TrackProducer`
Publishes sequential groups of data to a track.

```cpp
class TrackProducer {
public:
    ~TrackProducer();
    
    // Create a new group with given sequence number
    std::unique_ptr<GroupProducer> createGroup(uint64_t sequence);
};
```

#### `moq::TrackConsumer`
Consumes sequential groups of data from a track.

```cpp
class TrackConsumer {
public:
    ~TrackConsumer();
    
    // Asynchronously receive the next group
    std::future<std::unique_ptr<GroupConsumer>> nextGroup();
};
```

**Track Example:**
```cpp
// Define track metadata
moq::Track video_track = {"video", 0};      // High priority video
moq::Track audio_track = {"audio", 1};      // Lower priority audio
moq::Track metadata_track = {"metadata", 2}; // Lowest priority metadata

// Producer: Create groups with sequence numbers
auto group = track_producer->createGroup(42);

// Consumer: Process groups as they arrive
auto group_future = track_consumer->nextGroup();
auto group = group_future.get(); // Blocks until group arrives
```

### Group API

#### `moq::GroupProducer`
Publishes frames of data within a sequenced group.

```cpp
class GroupProducer {
public:
    ~GroupProducer();
    
    // Write frame data in different formats
    bool writeFrame(const std::vector<uint8_t>& data);
    bool writeFrame(const std::string& data);  
    bool writeFrame(const uint8_t* data, size_t size);
    
    // Signal end of group (required)
    void finish();
};
```

#### `moq::GroupConsumer`
Consumes frames of data within a received group.

```cpp
class GroupConsumer {
public:
    ~GroupConsumer();
    
    // Asynchronously read next frame from group
    std::future<std::optional<std::vector<uint8_t>>> readFrame();
};
```

**Group/Frame Example:**
```cpp
// Producer: Send multiple frames per group
auto group = track_producer->createGroup(sequence++);

group->writeFrame("Frame Header");
group->writeFrame(binary_data);
group->writeFrame(metadata_json);
group->finish(); // Always call finish()

// Consumer: Read all frames from group
while (true) {
    auto frame_future = group->readFrame();
    auto frame_data = frame_future.get();
    
    if (!frame_data) break; // No more frames
    
    // Process frame data
    processFrame(*frame_data);
}
```

### Error Handling

#### `moq::Result`
Enumeration for operation results.

```cpp
enum class Result {
    Success = 0,         // Operation completed successfully
    InvalidArgument = 1, // Invalid input parameters  
    NetworkError = 2,    // Network connectivity issues
    TlsError = 3,        // TLS/SSL certificate problems
    DnsError = 4,        // DNS resolution failures
    GeneralError = 5     // Other unspecified errors
};
```

**Error Handling Patterns:**
```cpp
// Check initialization result
auto result = moq::Client::initialize();
if (result != moq::Result::Success) {
    std::cerr << "Failed to initialize: " 
              << moq::Client::resultToString(result) << std::endl;
    return -1;
}

// Check connection result
auto session = client->connect(url);
if (!session) {
    std::cerr << "Connection failed: " << client->getLastError() << std::endl;
    return -1;
}

// Check publishing result
if (!session->publish(broadcast_name, producer)) {
    std::cerr << "Failed to publish broadcast" << std::endl;
}
```

## Development

### Project Structure

```
moq-c-api/
├── Cargo.toml              # Rust workspace configuration
├── CMakeLists.txt          # Main CMake configuration
├── moq-ffi/                # Rust FFI crate
│   ├── Cargo.toml
│   ├── build.rs            # cbindgen integration
│   └── src/
│       └── lib.rs          # C FFI implementation
├── cpp/                    # C++ wrapper
│   ├── include/moq/        # Public headers
│   └── src/                # Implementation
├── examples/               # Example applications
│   └── cpp/
└── README.md
```

### Building for Development

For development, you may want to build in debug mode:

```bash
cmake -B build -DCMAKE_BUILD_TYPE=Debug
cmake --build build
```

### Running Tests

```bash
# Build and run Rust tests
cd moq-ffi && cargo test

# Build and run C++ tests (if implemented)
cd build && ctest
```

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests if applicable
5. Submit a pull request

## License

This project is licensed under the same license as the original moq-rs project.

## Troubleshooting

### Common Issues

1. **Rust not found**: Ensure Rust is installed and in your PATH
2. **CMake version too old**: Update to CMake 3.16 or later
3. **Missing dependencies**: Install platform-specific build tools

### Debug Build

For debugging, build with debug symbols:

```bash
cmake -B build -DCMAKE_BUILD_TYPE=Debug -DCARGO_PROFILE=dev
cmake --build build
```

### Logging

The library uses tracing for logging. Set the `RUST_LOG` environment variable:

```bash
export RUST_LOG=debug
./moq_example
```

## Roadmap

- [ ] Streaming API for tracks
- [ ] Subscription management
- [ ] Publishing capabilities
- [ ] Advanced error handling
- [ ] Performance optimizations
- [ ] Additional platform support
