cmake_minimum_required(VERSION 3.16)
project(moq-c-api VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Find Rust and Cargo
find_program(CARGO_EXECUTABLE cargo REQUIRED)

# Build the Rust FFI library
set(RUST_TARGET_DIR ${CMAKE_CURRENT_BINARY_DIR}/rust-target)
set(RUST_LIB_DIR ${RUST_TARGET_DIR}/release)

if(WIN32)
    set(RUST_LIB_NAME moq_ffi.dll.lib)
    set(RUST_DLL_NAME moq_ffi.dll)
elseif(APPLE)
    set(RUST_LIB_NAME libmoq_ffi.dylib)
else()
    set(RUST_LIB_NAME libmoq_ffi.so)
endif()

# Custom command to build the Rust library
add_custom_command(
    OUTPUT ${RUST_LIB_DIR}/${RUST_LIB_NAME}
    COMMAND ${CARGO_EXECUTABLE} build --release --target-dir ${RUST_TARGET_DIR}
    WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}/moq-ffi
    COMMENT "Building Rust FFI library"
)

# Create a target for the Rust library
add_custom_target(rust_lib ALL DEPENDS ${RUST_LIB_DIR}/${RUST_LIB_NAME})

# Create an imported library target
add_library(moq_ffi SHARED IMPORTED)
set_target_properties(moq_ffi PROPERTIES
    IMPORTED_LOCATION ${RUST_LIB_DIR}/${RUST_LIB_NAME}
)

if(WIN32)
    set_target_properties(moq_ffi PROPERTIES
        IMPORTED_IMPLIB ${RUST_LIB_DIR}/${RUST_LIB_NAME}
        IMPORTED_LOCATION ${RUST_LIB_DIR}/${RUST_DLL_NAME}
    )
endif()

add_dependencies(moq_ffi rust_lib)

# C++ wrapper library
add_library(moq_cpp STATIC
    cpp/src/moq_client.cpp
    cpp/src/moq_session.cpp
)

target_include_directories(moq_cpp PUBLIC
    cpp/include
    ${RUST_TARGET_DIR}/release/moq-ffi/include  # Generated C headers
)

target_link_libraries(moq_cpp PUBLIC moq_ffi)

# Example executable
add_executable(moq_example examples/cpp/basic_example.cpp)
target_link_libraries(moq_example moq_cpp)

# Find threading library
find_package(Threads REQUIRED)
target_link_libraries(moq_cpp PUBLIC Threads::Threads)

# Platform-specific libraries
if(APPLE)
    target_link_libraries(moq_cpp PUBLIC "-framework Security" "-framework CoreFoundation")
elseif(UNIX)
    target_link_libraries(moq_cpp PUBLIC dl m)
elseif(WIN32)
    target_link_libraries(moq_cpp PUBLIC ws2_32 userenv advapi32)
endif()

# Installation
install(TARGETS moq_cpp moq_example
    LIBRARY DESTINATION lib
    ARCHIVE DESTINATION lib
    RUNTIME DESTINATION bin
)

install(DIRECTORY cpp/include/
    DESTINATION include
    FILES_MATCHING PATTERN "*.h" PATTERN "*.hpp"
)

install(FILES ${RUST_LIB_DIR}/${RUST_LIB_NAME}
    DESTINATION lib
)

if(WIN32 AND RUST_DLL_NAME)
    install(FILES ${RUST_LIB_DIR}/${RUST_DLL_NAME}
        DESTINATION bin
    )
endif()
