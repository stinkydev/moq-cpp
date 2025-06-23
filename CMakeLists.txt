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
    OUTPUT ${RUST_LIB_DIR}/${RUST_LIB_NAME} ${RUST_TARGET_DIR}/release/moq-ffi/include/moq_ffi.h
    COMMAND ${CARGO_EXECUTABLE} build --release --target-dir ${RUST_TARGET_DIR}
    COMMAND ${CMAKE_COMMAND} -E echo "Verifying generated files..."
    COMMAND ${CMAKE_COMMAND} -E ls ${RUST_LIB_DIR}/${RUST_LIB_NAME}
    COMMAND ${CMAKE_COMMAND} -E ls ${RUST_TARGET_DIR}/release/moq-ffi/include/moq_ffi.h
    COMMAND ${CMAKE_COMMAND} -E echo "Rust FFI build completed successfully"
    WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}/moq-ffi
    COMMENT "Building Rust FFI library and generating headers"
    VERBATIM
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

# Verify C++ headers exist
if(NOT EXISTS "${CMAKE_CURRENT_SOURCE_DIR}/cpp/include/moq/client.h")
    message(FATAL_ERROR "Required header cpp/include/moq/client.h not found")
endif()
if(NOT EXISTS "${CMAKE_CURRENT_SOURCE_DIR}/cpp/include/moq/session.h")
    message(FATAL_ERROR "Required header cpp/include/moq/session.h not found")
endif()
if(NOT EXISTS "${CMAKE_CURRENT_SOURCE_DIR}/cpp/include/moq/moq.h")
    message(FATAL_ERROR "Required header cpp/include/moq/moq.h not found")
endif()

# C++ wrapper library
add_library(moq_cpp STATIC
    cpp/src/moq_client.cpp
    cpp/src/moq_session.cpp
)

# Set include directories with absolute paths
target_include_directories(moq_cpp PUBLIC
    $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/cpp/include>
    $<BUILD_INTERFACE:${RUST_TARGET_DIR}/release/moq-ffi/include>
    $<INSTALL_INTERFACE:include>
)

# Debug include paths
message(STATUS "MOQ C++ include path: ${CMAKE_CURRENT_SOURCE_DIR}/cpp/include")
message(STATUS "MOQ FFI include path: ${RUST_TARGET_DIR}/release/moq-ffi/include")

# Ensure headers are generated before compiling C++
add_dependencies(moq_cpp rust_lib)

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
