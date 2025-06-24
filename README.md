# MOQ C++ API

A C++ wrapper around the [moq-native](https://github.com/kixelated/moq-rs) Rust library, providing a C++ interface for Media over QUIC (MOQ) applications.

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

## Usage

### Basic Example

```cpp
#include <moq/moq.h>
#include <iostream>

int main() {
    // Initialize the library
    auto result = moq::Client::initialize();
    if (result != moq::Result::Success) {
        std::cerr << "Failed to initialize: " 
                  << moq::Client::resultToString(result) << std::endl;
        return 1;
    }

    // Configure the client
    moq::ClientConfig config;
    config.bind_addr = "[::]:0";
    config.tls_disable_verify = false;

    // Create client
    auto client = moq::Client::create(config);
    if (!client) {
        std::cerr << "Failed to create client" << std::endl;
        return 1;
    }

    // Connect to server
    auto session = client->connect("https://moq.sesame-streams.com:4443");
    if (!session) {
        std::cerr << "Failed to connect" << std::endl;
        return 1;
    }

    std::cout << "Connected successfully!" << std::endl;
    return 0;
}
```

### Publishing Data (Clock Example)

```cpp
#include <moq/moq.h>
#include <iostream>
#include <chrono>
#include <thread>

int main() {
    // Initialize and connect (same as basic example)
    auto client = moq::Client::create(config);
    auto session = client->connect("https://moq.sesame-streams.com:4443");

    // Define track information
    moq::Track track;
    track.name = "seconds";
    track.priority = 0;

    // Create broadcast producer
    auto broadcast_producer = std::make_shared<moq::BroadcastProducer>();
    
    // Create track producer
    auto track_producer = broadcast_producer->createTrack(track);
    if (!track_producer) {
        std::cerr << "Failed to create track producer" << std::endl;
        return 1;
    }

    // Publish the broadcast
    if (!session->publish("clock", broadcast_producer->getConsumable())) {
        std::cerr << "Failed to publish broadcast" << std::endl;
        return 1;
    }

    // Publishing loop
    uint64_t sequence = 0;
    while (true) {
        // Create a new group
        auto group = track_producer->createGroup(sequence++);
        if (!group) {
            std::cerr << "Failed to create group" << std::endl;
            break;
        }

        // Write timestamp data
        auto now = std::chrono::system_clock::now();
        auto time_t = std::chrono::system_clock::to_time_t(now);
        std::string timestamp = std::ctime(&time_t);
        
        if (!group->writeFrame(timestamp)) {
            std::cerr << "Failed to write frame" << std::endl;
            break;
        }

        // Finish the group
        group->finish();

        // Wait before next update
        std::this_thread::sleep_for(std::chrono::seconds(1));
    }

    return 0;
}
```

### Consuming Data (Clock Example)

```cpp
#include <moq/moq.h>
#include <iostream>

int main() {
    // Initialize and connect (same as basic example)
    auto client = moq::Client::create(config);
    auto session = client->connect("https://moq.sesame-streams.com:4443");

    // Define track to subscribe to
    moq::Track track;
    track.name = "seconds";
    track.priority = 0;

    // Consume the broadcast
    auto broadcast_consumer = session->consume("clock");
    if (!broadcast_consumer) {
        std::cerr << "Failed to consume broadcast" << std::endl;
        return 1;
    }

    // Subscribe to the track
    auto track_consumer = broadcast_consumer->subscribeTrack(track);
    if (!track_consumer) {
        std::cerr << "Failed to subscribe to track" << std::endl;
        return 1;
    }

    // Consumption loop
    while (true) {
        // Get next group
        auto group_future = track_consumer->nextGroup();
        auto group = group_future.get();
        
        if (!group) {
            std::cout << "No more groups available" << std::endl;
            break;
        }

        // Read frames from the group
        while (true) {
            auto frame_future = group->readFrame();
            auto frame_data = frame_future.get();
            
            if (!frame_data) {
                break; // No more frames in this group
            }

            // Convert to string and display
            std::string timestamp(frame_data->begin(), frame_data->end());
            std::cout << "Received: " << timestamp << std::endl;
        }
    }

    return 0;
}
```

### Advanced Usage: Multi-Frame Groups

```cpp
// Publishing multiple frames per group
auto group = track_producer->createGroup(sequence_number);

// Write base data
group->writeFrame("Header: ");

// Write multiple data frames
for (int i = 0; i < 10; i++) {
    std::string data = "Frame " + std::to_string(i);
    group->writeFrame(data);
}

// Always finish the group
group->finish();
```

```cpp
// Consuming multiple frames per group
auto group = group_future.get();

// Read the header frame
auto header_future = group->readFrame();
auto header_data = header_future.get();
std::string header(header_data->begin(), header_data->end());

// Read remaining frames
while (true) {
    auto frame_future = group->readFrame();
    auto frame_data = frame_future.get();
    
    if (!frame_data) break; // No more frames
    
    std::string frame(frame_data->begin(), frame_data->end());
    std::cout << header << frame << std::endl;
}
```

### Integration with Existing Projects

#### Using CMake FetchContent

```cmake
include(FetchContent)

FetchContent_Declare(
    moq-c-api
    GIT_REPOSITORY <your-repo-url>
    GIT_TAG main
)

FetchContent_MakeAvailable(moq-c-api)

target_link_libraries(your_target PRIVATE moq_cpp)
```

#### Using find_package (after installation)

```cmake
find_package(moq-c-api REQUIRED)
target_link_libraries(your_target PRIVATE moq::moq_cpp)
```

## API Reference

### Core Classes

#### `moq::Client`
Main client class for MOQ connections.

**Methods:**
- `static Result initialize()` - Initialize the library (call once)
- `static std::unique_ptr<Client> create(const ClientConfig&)` - Create a new client
- `std::unique_ptr<Session> connect(const std::string& url)` - Connect to a server
- `std::string getLastError() const` - Get the last error message
- `static std::string resultToString(Result)` - Convert result code to string

#### `moq::Session`
Represents an active connection to a MOQ server.

**Methods:**
- `bool isConnected() const` - Check if the session is active
- `void close()` - Close the session
- `bool publish(const std::string& broadcast_name, std::shared_ptr<BroadcastProducer> producer)` - Publish a broadcast
- `std::unique_ptr<BroadcastConsumer> consume(const std::string& broadcast_name)` - Consume a broadcast

#### `moq::ClientConfig`
Configuration structure for the client.

**Fields:**
- `std::string bind_addr` - Local bind address (default: "[::]:0")
- `bool tls_disable_verify` - Disable TLS verification (default: false)
- `std::string tls_root_cert_path` - Path to custom root certificate

### Broadcast API

#### `moq::BroadcastProducer`
Manages publishing multiple tracks within a broadcast.

**Methods:**
- `BroadcastProducer()` - Constructor
- `~BroadcastProducer()` - Destructor
- `std::unique_ptr<TrackProducer> createTrack(const Track& track)` - Create a track producer for the given track
- `std::shared_ptr<BroadcastProducer> getConsumable()` - Get a consumable version of this producer for publishing

#### `moq::BroadcastConsumer`
Manages consuming multiple tracks from a broadcast.

**Methods:**
- `~BroadcastConsumer()` - Destructor  
- `std::unique_ptr<TrackConsumer> subscribeTrack(const Track& track)` - Subscribe to a specific track in the broadcast

### Track API

#### `moq::Track`
Track information structure.

**Fields:**
- `std::string name` - Track name
- `uint8_t priority` - Track priority (default: 0)

#### `moq::TrackProducer`
Publishes data to a track organized in groups.

**Methods:**
- `~TrackProducer()` - Destructor
- `std::unique_ptr<GroupProducer> createGroup(uint64_t sequence_number)` - Create a new group for publishing data

#### `moq::TrackConsumer`
Consumes data from a track organized in groups.

**Methods:**
- `~TrackConsumer()` - Destructor
- `std::future<std::unique_ptr<GroupConsumer>> nextGroup()` - Get the next group of data from the track

### Group API

#### `moq::GroupProducer`
Publishes frame data within a group.

**Methods:**
- `~GroupProducer()` - Destructor
- `bool writeFrame(const std::vector<uint8_t>& data)` - Write frame data as bytes
- `bool writeFrame(const std::string& data)` - Write frame data as string
- `bool writeFrame(const uint8_t* data, size_t size)` - Write frame data from raw pointer
- `void finish()` - Mark group as complete (no more frames will be written)

#### `moq::GroupConsumer`
Consumes frame data within a group.

**Methods:**
- `~GroupConsumer()` - Destructor
- `std::future<std::optional<std::vector<uint8_t>>> readFrame()` - Read the next frame from the group

### Error Handling

The library uses the `moq::Result` enum for error reporting:

- `Success` - Operation completed successfully
- `InvalidArgument` - Invalid input parameters
- `NetworkError` - Network-related error
- `TlsError` - TLS/SSL error
- `DnsError` - DNS resolution error
- `GeneralError` - Other error

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
