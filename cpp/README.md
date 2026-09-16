# MOQ C++ Wrapper

This directory contains C++ bindings for the MOQ (Media over QUIC) wrapper library.

## Building

### Prerequisites

- CMake 3.16 or later
- A C++17 compatible compiler (GCC 7+, Clang 5+, MSVC 2017+)
- Rust toolchain (for building the underlying library)

### Build Instructions

```bash
# Create build directory
mkdir build && cd build

# Configure with CMake
cmake .. -DCMAKE_BUILD_TYPE=Release

# Build the library
cmake --build .

# Optionally build examples
cmake .. -DCMAKE_BUILD_TYPE=Release
cmake --build .
```

### Installation

```bash
# Install the library
cmake --install . --prefix /usr/local
```

## Usage

### Basic Setup

```cpp
#include "moq_wrapper.h"

moq::SetLogLevel(moq::LogLevel::kInfo);
```

### Creating a Publisher

```cpp
// Define tracks
std::vector<moq::TrackDefinition> tracks;
tracks.emplace_back("video", 0, moq::TrackType::kVideo);
tracks.emplace_back("audio", 1, moq::TrackType::kAudio);

// Create publisher session
auto session = moq::Session::CreatePublisher(
    "https://relay.quic.video:4443", 
    "my-broadcast", 
    tracks,
    moq::CatalogType::kHang
);

// Wait for connection
while (!session->IsConnected()) {
    std::this_thread::sleep_for(std::chrono::milliseconds(100));
}

// Publish data
session->PublishData("video", data, size);
```

### Creating a Subscriber

```cpp
// Define tracks to subscribe to
std::vector<moq::TrackDefinition> tracks;
tracks.emplace_back("video", 0, moq::TrackType::kVideo);
tracks.emplace_back("audio", 1, moq::TrackType::kAudio);

// Create subscriber session
auto session = moq::Session::CreateSubscriber(
    "https://relay.quic.video:4443", 
    "my-broadcast", 
    tracks,
    moq::CatalogType::kHang,
    false
);

// Set data callback
session->SetDataCallback([](const std::string& track, 
                           const uint8_t* data, 
                           size_t size) {
    std::cout << "Received " << size << " bytes on track: " << track << std::endl;
});

// Wait for connection
while (!session->IsConnected()) {
    std::this_thread::sleep_for(std::chrono::milliseconds(100));
}
```

To subscribe to every track listed in `catalog.json`, pass an empty explicit
track list and set the last argument to `true`:

```cpp
std::vector<moq::TrackDefinition> tracks;

auto session = moq::Session::CreateSubscriber(
    "https://relay.quic.video:4443",
    "my-broadcast",
    tracks,
    moq::CatalogType::kHang,
    true
);
```

### Creating a Room Subscriber

```cpp
std::vector<moq::TrackDefinition> tracks;
tracks.emplace_back("clock", 0, moq::TrackType::kData);

auto session = moq::Session::CreateRoomSubscriber(
    "https://r2.moq.sesame-streams.com:4433",
    "my-room",
    tracks,
    moq::CatalogType::kNone,
    false
);

session->SetDataCallback([](const std::string& track,
                            const uint8_t* data,
                            size_t size) {
    // In room mode, track is "broadcast_path/track_name".
    std::cout << "Received " << size << " bytes on " << track << std::endl;
});
```

## Examples

The examples are provided as a separate CMake project in the `examples/` directory. They demonstrate how to use the library as an external dependency:

```bash
# First install the library
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

### Clock Publisher

```bash
./clock_publisher_example [url] [broadcast_name] [publisher_count] [interval_ms]
```

Example:
```bash
./clock_publisher_example https://r2.moq.sesame-streams.com:4433 my-room 3 1000
```

### Clock Subscriber

```bash
./clock_subscriber_example [url] [broadcast_or_room_prefix] [exact|room] [track] [none|sesame|hang] [all_catalog_tracks]
```

Example:
```bash
./clock_subscriber_example https://r2.moq.sesame-streams.com:4433 my-room room
./clock_subscriber_example https://r2.moq.sesame-streams.com:4433 my-room room clock sesame true
```

## Code Style

This project follows the Google C++ Style Guide:
- Use snake_case for variables and functions
- Use PascalCase for classes and enums
- Use k prefix for enum values
- Use trailing underscores for private member variables
- Maximum line length of 80 characters where practical

## API Reference

### Classes

#### `moq::TrackDefinition`
Represents a track definition with name, priority, and type.

#### `moq::Session`
Main session class for MOQ operations. Use static factory methods to create instances.

### Enums

#### `moq::LogLevel`
- `kTrace`, `kDebug`, `kInfo`, `kWarn`, `kError`

#### `moq::TrackType`
- `kVideo`, `kAudio`, `kData`

#### `moq::CatalogType`
- `kNone`, `kSesame`, `kHang`

### Functions

#### `moq::SetLogLevel(LogLevel)`
Initialize global library logging.

## Threading

The C++ wrapper handles threading internally using the Rust async runtime. All callbacks are executed on background threads, so ensure thread safety in your callback implementations.

## Memory Management

The wrapper uses RAII principles:
- `TrackDefinition` objects manage their own FFI handles
- `Session` objects automatically clean up on destruction
- Use `std::unique_ptr` for session management

## Error Handling

Functions return `nullptr` or `false` on error. Check return values and connection status regularly.
