# MOQ C++ API - Publisher/Subscriber Example

This example demonstrates the extended MOQ C++ API with publishing and subscribing functionality.

## What Was Added

### 1. Rust FFI Layer Extensions (`moq-ffi/src/lib.rs`)
- **Track Management**: Added `MoqTrack` struct and related functions
- **Publishing**: `moq_session_publish_track()` and `moq_track_send_data()`
- **Subscribing**: `moq_session_subscribe_track()` with callback support
- **Memory Management**: Proper cleanup with `moq_track_free()`

### 2. C++ Wrapper Extensions
- **Track Class** (`cpp/include/moq/track.h`, `cpp/src/moq_track.cpp`):
  - RAII-compliant Track management
  - Multiple `sendData()` overloads (vector, raw pointer, string)
  - Publisher/subscriber differentiation

- **Session Extensions** (`cpp/include/moq/session.h`, `cpp/src/moq_session.cpp`):
  - `publishTrack()` method
  - `subscribeTrack()` method with C++ callback support
  - Automatic callback management

### 3. Example Applications
- **Basic Example** (`examples/cpp/basic_example.cpp`): 
  - Demonstrates simple publisher/subscriber setup
  - Single message exchange

- **Comprehensive Example** (`examples/cpp/publisher_subscriber_example.cpp`):
  - Multiple message types (text and binary)
  - Detailed logging and status reporting
  - Demonstrates full API capabilities

## API Usage

### Creating Clients
```cpp
// Create separate clients for publishing and subscribing
auto publisher_client = moq::Client::create(pub_config);
auto subscriber_client = moq::Client::create(sub_config);
```

### Establishing Sessions
```cpp
auto publisher_session = publisher_client->connect(server_url);
auto subscriber_session = subscriber_client->connect(server_url);
```

### Setting Up Subscriber
```cpp
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

# Run the basic example
./moq_example

# Run the comprehensive example  
./moq_pub_sub_example
```

## Architecture Notes

- **Memory Safety**: All resources are managed through RAII principles
- **Type Safety**: Strong typing with proper C++ abstractions over C FFI
- **Error Handling**: Comprehensive error checking at both FFI and C++ levels
- **Callback Management**: Safe callback storage and execution
- **Thread Safety**: Proper synchronization for global state management

This implementation provides a solid foundation for MOQ applications with clean, modern C++ interfaces over the Rust MOQ native library.
