<!-- Use this file to provide workspace-specific custom instructions to Copilot. For more details, visit https://code.visualstudio.com/docs/copilot/copilot-customization#_use-a-githubcopilotinstructionsmd-file -->

# MOQ C++ API Development Instructions

This project provides a C++ wrapper around the Rust moq-native library for Media over QUIC (MOQ) applications.

## Project Architecture

- **moq-ffi/**: Rust crate that exposes C FFI bindings around moq-native
- **cpp/**: C++ wrapper that provides RAII and modern C++ semantics
- **examples/**: Sample applications demonstrating usage
- **CMake**: Build system for the C++ components

## Development Guidelines

### Rust FFI Layer (moq-ffi/)
- Use `#[no_mangle]` for all exported functions
- Follow C naming conventions (snake_case)
- Always validate null pointers before dereferencing
- Use proper error handling with the MoqResult enum
- Document all public functions with /// comments
- Use cbindgen for automatic header generation

### C++ Wrapper Layer (cpp/)
- Follow RAII principles - resources are managed by destructors
- Use smart pointers (std::unique_ptr) for ownership
- Provide exception-safe interfaces
- Use modern C++17 features appropriately
- Keep headers minimal and include what you use

### Error Handling
- Rust layer returns MoqResult enum values
- C++ layer translates to exceptions or optional return values
- Provide descriptive error messages through getLastError()

### Memory Management
- Rust layer: Use Box::into_raw() and Box::from_raw() for C interop
- C++ layer: Use RAII and smart pointers exclusively
- Never mix malloc/free with Rust allocation

### Threading
- The Rust layer uses Tokio runtime for async operations
- C++ layer blocks on async operations for simplicity
- Consider async C++ interfaces for future versions

### Testing
- Write unit tests for both Rust and C++ layers
- Use integration tests to verify the full stack
- Test error conditions and edge cases

## Building and Dependencies

- Requires Rust toolchain with Cargo
- Requires CMake 3.16+ and C++17 compiler
- Uses cbindgen for header generation
- Links against system libraries (Security framework on macOS, etc.)

## Coding Standards

- Use rustfmt for Rust code formatting
- Use clang-format for C++ code formatting
- Follow conventional naming: snake_case for Rust, PascalCase for C++ classes
- Document public APIs thoroughly
- Include examples in documentation
