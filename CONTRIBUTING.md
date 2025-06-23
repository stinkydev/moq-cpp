# Contributing to MOQ C++ API

Thank you for your interest in contributing to the MOQ C++ API! This document provides guidelines and information for contributors.

## Development Environment

### Prerequisites

- **Rust**: Install from [rustup.rs](https://rustup.rs/)
- **CMake**: Version 3.16 or later
- **C++ Compiler**: Supporting C++17 or later (GCC 7+, Clang 5+, MSVC 2019+)
- **Git**: For version control

### Quick Start

1. **Clone the repository**:
   ```bash
   git clone <repository-url>
   cd moq-c-api
   ```

2. **Build the project**:
   ```bash
   ./scripts/build.sh debug
   ```

3. **Run the example**:
   ```bash
   ./scripts/build.sh example
   ```

## Project Structure

```
moq-c-api/
├── moq-ffi/          # Rust FFI wrapper around moq-native
├── cpp/              # C++ wrapper library
├── examples/         # Example applications
├── scripts/          # Development scripts
├── .github/          # GitHub Actions CI/CD
└── CMakeLists.txt    # Main build configuration
```

## Development Guidelines

### Rust FFI Layer (moq-ffi/)

- Use `#[no_mangle]` for all exported functions
- Follow C naming conventions (snake_case)
- Always validate null pointers before dereferencing
- Use proper error handling with the `MoqResult` enum
- Document all public functions with `///` comments
- Use cbindgen for automatic header generation

Example:
```rust
/// Create a new MOQ client
/// 
/// # Arguments
/// * `config` - Configuration for the client
/// * `client_out` - Output parameter for the created client handle
/// 
/// # Returns
/// * `MoqResult` indicating success or failure
#[no_mangle]
pub extern "C" fn moq_client_new(
    config: *const MoqClientConfig,
    client_out: *mut *mut MoqClient,
) -> MoqResult {
    // Implementation...
}
```

### C++ Wrapper Layer (cpp/)

- Follow RAII principles - resources are managed by destructors
- Use smart pointers (`std::unique_ptr`) for ownership
- Provide exception-safe interfaces
- Use modern C++17 features appropriately
- Keep headers minimal and include what you use

Example:
```cpp
class Client {
public:
    static std::unique_ptr<Client> create(const ClientConfig& config);
    std::unique_ptr<Session> connect(const std::string& url);
    ~Client(); // RAII cleanup
private:
    Client(void* handle); // Private constructor
    void* handle_;
};
```

### Error Handling

- **Rust layer**: Returns `MoqResult` enum values
- **C++ layer**: Translates to exceptions or optional return values
- Provide descriptive error messages through `getLastError()`

### Memory Management

- **Rust layer**: Use `Box::into_raw()` and `Box::from_raw()` for C interop
- **C++ layer**: Use RAII and smart pointers exclusively
- Never mix malloc/free with Rust allocation

## Building and Testing

### Build Commands

```bash
# Debug build
./scripts/build.sh debug

# Release build
./scripts/build.sh release

# Clean build
./scripts/build.sh clean

# Build and run example
./scripts/build.sh example

# Install to system
./scripts/build.sh install
```

### Running Tests

```bash
# Rust tests
cargo test --manifest-path moq-ffi/Cargo.toml

# C++ integration tests (run example)
./scripts/build.sh example
```

### Code Formatting

```bash
# Rust formatting
cargo fmt --manifest-path moq-ffi/Cargo.toml

# Rust linting
cargo clippy --manifest-path moq-ffi/Cargo.toml

# C++ formatting (if clang-format is available)
find cpp/ -name "*.cpp" -o -name "*.h" | xargs clang-format -i
```

## Submitting Changes

### Pull Request Process

1. **Fork the repository** and create a feature branch
2. **Make your changes** following the coding guidelines
3. **Add tests** if applicable
4. **Update documentation** if you change the API
5. **Ensure all tests pass** locally
6. **Submit a pull request** with a clear description

### Commit Messages

Use conventional commit format:
```
type(scope): description

feat(ffi): add session management functions
fix(cpp): handle null pointer in Client::connect
docs(readme): update build instructions
```

### Code Review

- All changes require review before merging
- Address feedback promptly
- Keep pull requests focused and atomic
- Rebase on main before merging

## API Design Principles

### Rust FFI Layer

1. **Safety First**: Always validate inputs and handle errors gracefully
2. **C Compatibility**: Use C-compatible types and calling conventions
3. **Clear Ownership**: Make resource ownership explicit
4. **Error Reporting**: Use consistent error codes and messages

### C++ Wrapper Layer

1. **RAII**: Automatic resource management through destructors
2. **Modern C++**: Use C++17 features where appropriate
3. **Exception Safety**: Provide strong exception safety guarantees
4. **Intuitive API**: Make the interface natural for C++ developers

## Documentation

- **Rust code**: Use `///` comments for public APIs
- **C++ code**: Use Doxygen-style comments
- **Examples**: Provide working examples for new features
- **README**: Keep the main README up to date

## Getting Help

- **Issues**: Create GitHub issues for bugs or feature requests
- **Discussions**: Use GitHub Discussions for questions
- **Code Review**: Participate in pull request reviews

## License

By contributing to this project, you agree that your contributions will be licensed under the MIT License.
