# MOQ C++ API

A C++ wrapper around the [moq-native](https://github.com/kixelated/moq-rs) Rust library, providing a convenient C++ interface for Media over QUIC (MOQ) applications.

## Overview

This project provides:
- **Rust FFI layer** (`moq-ffi`): C-compatible interface around moq-native
- **C++ wrapper** (`cpp/`): Modern C++ classes and RAII semantics
- **CMake build system**: Easy integration into C++ projects
- **Examples**: Sample code demonstrating usage

## Architecture

```
┌─────────────────┐
│   C++ Application   │
├─────────────────┤
│   C++ Wrapper       │  <- moq::Client, moq::Session classes
├─────────────────┤
│   C FFI Layer       │  <- moq_client_new, moq_client_connect, etc.
├─────────────────┤
│   Rust moq-native   │  <- Original moq-native library
└─────────────────┘
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

### Quick Start

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
    auto session = client->connect("https://relay.quic.video:443");
    if (!session) {
        std::cerr << "Failed to connect" << std::endl;
        return 1;
    }

    std::cout << "Connected successfully!" << std::endl;
    return 0;
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

### Classes

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

#### `moq::ClientConfig`
Configuration structure for the client.

**Fields:**
- `std::string bind_addr` - Local bind address (default: "[::]:0")
- `bool tls_disable_verify` - Disable TLS verification (default: false)
- `std::string tls_root_cert_path` - Path to custom root certificate

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
