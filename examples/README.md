# MOQ C++ Examples

This directory contains example applications demonstrating how to use the MOQ C++ library.

## Prerequisites

1. Install the MOQ C++ library first:
   ```bash
   # From the project root
   cmake -B build -DCMAKE_BUILD_TYPE=Release
   cmake --build build
   cmake --install build --prefix /usr/local  # or your preferred prefix
   ```

2. Make sure you have CMake 3.16+ and a C++17 compatible compiler.

## Building the Examples

```bash
cd examples
cmake -B build -DCMAKE_BUILD_TYPE=Release
cmake --build build
```

If you installed the library to a custom location, specify the prefix:
```bash
cmake -B build -DCMAKE_BUILD_TYPE=Release -DCMAKE_PREFIX_PATH=/path/to/install
cmake --build build
```

## Running the Examples

### Clock Publisher
```bash
# Use default relay and broadcast name
./build/clock_publisher_example

# Specify custom relay and broadcast name
./build/clock_publisher_example https://relay.example.com:4443 my-broadcast
```

### Clock Subscriber  
```bash
# Use default relay and broadcast name
./build/clock_subscriber_example

# Specify custom relay and broadcast name
./build/clock_subscriber_example https://relay.example.com:4443 my-broadcast
```

## Examples Description

- **clock_publisher.cpp**: Publishes current time data every second over MOQ
- **clock_subscriber.cpp**: Subscribes to and displays time data from a MOQ broadcast

These examples demonstrate the basic publisher/subscriber pattern using the MOQ protocol.