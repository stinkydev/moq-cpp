# MoQ Examples

This directory contains Rust and C++ examples for the current wrapper API.

The examples default to:

```text
https://r2.moq.sesame-streams.com:4433
```

## Rust Wrapper Clock

Run one publisher on a single broadcast:

```bash
cargo run --example clock_example -- --broadcast clock-rust publish
```

Run three publishers under one room prefix:

```bash
cargo run --example clock_example -- --broadcast clock-room publish --publishers 3
```

Subscribe to one exact broadcast:

```bash
cargo run --example clock_example -- --broadcast clock-rust subscribe
```

Subscribe using every track listed in the publisher's catalog:

```bash
cargo run --example clock_example -- --broadcast clock-rust --catalog sesame subscribe --all-catalog-tracks
```

Subscribe to every announced broadcast under a room prefix:

```bash
cargo run --example clock_example -- --broadcast clock-room subscribe --room
```

Subscribe to every catalog track in every announced broadcast under a room prefix:

```bash
cargo run --example clock_example -- --broadcast clock-room --catalog sesame subscribe --room --all-catalog-tracks
```

Run a self-contained exact-broadcast smoke test for catalog-discovered tracks:

```bash
cargo run --example clock_example -- --broadcast clock-smoke catalog-smoke
```

Run a self-contained room smoke test with multiple catalog publishers:

```bash
cargo run --example clock_example -- --broadcast clock-smoke-room catalog-smoke --room --publishers 3
```

In room mode, data callbacks receive track names in the form:

```text
broadcast_path/track_name
```

## Rust Hang Subscriber

Exact broadcast:

```bash
cargo run --example hang_subscriber -- --broadcast me
```

Room prefix:

```bash
cargo run --example hang_subscriber -- --broadcast room-prefix --room
```

Catalog-driven room subscription:

```bash
cargo run --example hang_subscriber -- --broadcast room-prefix --room --all-catalog-tracks
```

## Native Rust MoQ Clock

The nested `moq-clock` sample uses `moq-native 0.19` directly.

```bash
cargo run --manifest-path examples/moq-clock/Cargo.toml -- --broadcast clock-native publish
cargo run --manifest-path examples/moq-clock/Cargo.toml -- --broadcast clock-native subscribe
```

Room test with multiple publishers:

```bash
cargo run --manifest-path examples/moq-clock/Cargo.toml -- --broadcast native-room publish --publishers 3
cargo run --manifest-path examples/moq-clock/Cargo.toml -- --broadcast native-room subscribe --room
```

## C++ Examples

Build and install the library first:

```bash
cmake -B build -DCMAKE_BUILD_TYPE=Release
cmake --build build --config Release
cmake --install build --prefix install
```

Then build the examples:

```bash
cmake -B build-examples -S examples -DCMAKE_PREFIX_PATH=../install
cmake --build build-examples --config Release
```

Run one publisher:

```bash
./build-examples/clock_publisher_example
```

Run three publishers under one room prefix:

```bash
./build-examples/clock_publisher_example https://r2.moq.sesame-streams.com:4433 cpp-room 3 1000
```

Subscribe to one exact broadcast:

```bash
./build-examples/clock_subscriber_example https://r2.moq.sesame-streams.com:4433 clock-cpp exact
```

Subscribe using every track listed in the catalog:

```bash
./build-examples/clock_subscriber_example https://r2.moq.sesame-streams.com:4433 clock-cpp exact clock sesame true
```

Subscribe to every announced broadcast under a room prefix:

```bash
./build-examples/clock_subscriber_example https://r2.moq.sesame-streams.com:4433 cpp-room room
```

Subscribe to every catalog track in every announced broadcast under a room prefix:

```bash
./build-examples/clock_subscriber_example https://r2.moq.sesame-streams.com:4433 cpp-room room clock sesame true
```

On Windows, the built executables are usually under `build-examples/Release/`.
