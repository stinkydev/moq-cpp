cmake_minimum_required(VERSION 3.16)
project(moq-c-api VERSION 0.1.3 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Find Rust and Cargo
find_program(CARGO_EXECUTABLE cargo REQUIRED)

# Build the Rust FFI library
set(RUST_TARGET_DIR ${CMAKE_CURRENT_BINARY_DIR}/rust-target)

# Choose debug or release based on CMAKE_CONFIGURATION_TYPES
if(CMAKE_CONFIGURATION_TYPES)
    # Multi-config generator (like Visual Studio)
    # We need to handle both Debug and Release configurations
    # For now, force debug mode since it works correctly
    set(RUST_BUILD_TYPE "")
    set(RUST_LIB_DIR ${RUST_TARGET_DIR}/debug)
else()
    # Single-config generator (like Unix Makefiles or Ninja)
    if(CMAKE_BUILD_TYPE STREQUAL "Debug")
        set(RUST_BUILD_TYPE "")
        set(RUST_LIB_DIR ${RUST_TARGET_DIR}/debug)
    else()
        # For single-config release, still use debug since it works
        set(RUST_BUILD_TYPE "")
        set(RUST_LIB_DIR ${RUST_TARGET_DIR}/debug)
    endif()
endif()

if(WIN32)
    set(RUST_LIB_NAME moq_ffi.lib)
elseif(APPLE)
    set(RUST_LIB_NAME libmoq_ffi.a)
else()
    set(RUST_LIB_NAME libmoq_ffi.a)
endif()

# Custom command to build the Rust library
add_custom_command(
    OUTPUT ${RUST_LIB_DIR}/${RUST_LIB_NAME}
    COMMAND ${CARGO_EXECUTABLE} build ${RUST_BUILD_TYPE} --target-dir ${RUST_TARGET_DIR}
    COMMAND ${CMAKE_COMMAND} -E echo "Rust FFI build completed successfully"
    WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}/moq-ffi
    COMMENT "Building Rust FFI library (Debug=${RUST_BUILD_TYPE})"
    VERBATIM
)

# Create a target for the Rust library
add_custom_target(rust_lib ALL DEPENDS ${RUST_LIB_DIR}/${RUST_LIB_NAME})

# Create an imported library target
add_library(moq_ffi STATIC IMPORTED)
set_target_properties(moq_ffi PROPERTIES
    IMPORTED_LOCATION ${RUST_LIB_DIR}/${RUST_LIB_NAME}
)

add_dependencies(moq_ffi rust_lib)

# C++ wrapper library
add_library(moq_cpp STATIC
    moq-cpp/src/moq_client.cpp
    moq-cpp/src/moq_session.cpp
    moq-cpp/src/moq_track.cpp
    moq-cpp/src/moq_broadcast.cpp
    moq-cpp/src/moq_group.cpp
    moq-cpp/src/moq_origin.cpp
)

# Set include directories with absolute paths
target_include_directories(moq_cpp PUBLIC
    $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/moq-cpp/include>
    $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/moq-ffi/include>
    $<INSTALL_INTERFACE:include>
)

# Debug include paths
message(STATUS "MOQ C++ include path: ${CMAKE_CURRENT_SOURCE_DIR}/moq-cpp/include")
message(STATUS "MOQ FFI include path: ${RUST_TARGET_DIR}/release/moq-ffi/include")

# Ensure headers are generated before compiling C++
add_dependencies(moq_cpp rust_lib)

target_link_libraries(moq_cpp PUBLIC moq_ffi)

add_library(moq_mgr STATIC
    moq-mgr/src/moq-mgr-session.cc
    moq-mgr/src/moq-mgr-producer.cc
    moq-mgr/src/moq-mgr-consumer.cc
)

target_include_directories(moq_mgr PUBLIC
    $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/moq-mgr/include>
    $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/moq-cpp/include>
    $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/moq-ffi/include>
    $<INSTALL_INTERFACE:include>
)

add_dependencies(moq_mgr moq_cpp)
target_link_libraries(moq_mgr PUBLIC moq_cpp)

# Example executables
# Note: Old examples are temporarily disabled due to API changes
# add_executable(moq_example examples/cpp/basic_example.cpp)
# target_link_libraries(moq_example moq_cpp)

# add_executable(moq_pub_sub_example examples/cpp/publisher_subscriber_example.cpp)
# target_link_libraries(moq_pub_sub_example moq_cpp)

add_executable(moq_clock_example examples/cpp/clock_example.cpp)
target_link_libraries(moq_clock_example moq_cpp moq_ffi)

# Relay test is Windows-only due to use of conio.h
if(WIN32)
    add_executable(moq_relay_test examples/cpp/relay_test.cpp)
    target_link_libraries(moq_relay_test moq_cpp)
    
    # Relay test using MOQ Manager abstraction
    add_executable(moq_relay_test_mgr examples/cpp/relay_test_mgr.cpp)
    target_link_libraries(moq_relay_test_mgr moq_mgr)
endif()

# Test executable for CI
add_executable(moq_test test/basic_test.cpp)
target_link_libraries(moq_test moq_cpp)

# Find threading library
find_package(Threads REQUIRED)
target_link_libraries(moq_cpp PUBLIC Threads::Threads)

# Platform-specific libraries
if(APPLE)
    target_link_libraries(moq_cpp PUBLIC "-framework Security" "-framework CoreFoundation")
elseif(UNIX)
    target_link_libraries(moq_cpp PUBLIC dl m)
elseif(WIN32)
    # Windows system libraries needed for Rust dependencies (schannel, rustls_platform_verifier)
    target_link_libraries(moq_cpp PUBLIC 
        ws2_32 userenv advapi32 ntdll kernel32 bcrypt 
        crypt32 secur32 ncrypt ole32 shell32
    )
endif()

# Installation
install(TARGETS moq_cpp moq_mgr
    LIBRARY DESTINATION lib
    ARCHIVE DESTINATION lib
    RUNTIME DESTINATION bin
)

# Install C++ wrapper headers
install(DIRECTORY moq-cpp/include/
    DESTINATION include
    FILES_MATCHING PATTERN "*.h" PATTERN "*.hpp"
)

# Install MOQ Manager headers
install(DIRECTORY moq-mgr/include/
    DESTINATION include
    FILES_MATCHING PATTERN "*.h" PATTERN "*.hpp"
)

# Install FFI header
install(FILES moq-ffi/include/moq_ffi.h
    DESTINATION include
)

# Install Rust library
install(FILES ${RUST_LIB_DIR}/${RUST_LIB_NAME}
    DESTINATION lib
)

