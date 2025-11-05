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
cmake .. -DCMAKE_BUILD_TYPE=Release -DBUILD_EXAMPLES=ON
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

// Initialize the library with logging
moq::Init(moq::LogLevel::kInfo, [](const std::string& target, 
                                   moq::LogLevel level, 
                                   const std::string& message) {
    std::cout << "[" << level << "] " << target << ": " << message << std::endl;
});
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

// Publish data (implementation needed)
// session->PublishData("video", data, size);
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
    moq::CatalogType::kHang
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

## Examples

The examples are built when `BUILD_EXAMPLES=ON` is set during CMake configuration.

### Clock Publisher

```bash
./clock_publisher_cpp [url] [broadcast_name]
```

Example:
```bash
./clock_publisher_cpp https://relay.quic.video:4443 my-clock
```

### Clock Subscriber

```bash
./clock_subscriber_cpp [url] [broadcast_name]
```

Example:
```bash
./clock_subscriber_cpp https://relay.quic.video:4443 my-clock
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

#### `moq::Init(LogLevel, LogCallback)`
Initialize the MOQ library with logging configuration.

## Threading

The C++ wrapper handles threading internally using the Rust async runtime. All callbacks are executed on background threads, so ensure thread safety in your callback implementations.

## Memory Management

The wrapper uses RAII principles:
- `TrackDefinition` objects manage their own FFI handles
- `Session` objects automatically clean up on destruction
- Use `std::unique_ptr` for session management

## Error Handling

Functions return `nullptr` or `false` on error. Check return values and connection status regularly.