cmake_minimum_required(VERSION 3.16)
project(moq-cpp VERSION 0.1.0)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Fix RPATH behavior for proper library linking
set(CMAKE_MACOSX_RPATH ON)
set(CMAKE_INSTALL_RPATH "@loader_path")
set(CMAKE_BUILD_WITH_INSTALL_RPATH OFF)

# Linux equivalent
if(CMAKE_SYSTEM_NAME STREQUAL "Linux")
    set(CMAKE_INSTALL_RPATH "$ORIGIN")
endif()

# Find Rust and Cargo
find_program(CARGO_EXECUTABLE cargo REQUIRED)
find_program(RUSTC_EXECUTABLE rustc REQUIRED)

# Determine the Rust target directory and library name
# The .cargo/config.toml redirects target-dir to build/rust-target
if(CMAKE_BUILD_TYPE STREQUAL "Debug")
    set(RUST_TARGET_DIR "${CMAKE_CURRENT_SOURCE_DIR}/build/rust-target/debug")
    set(RUST_BUILD_FLAG "")
else()
    set(RUST_TARGET_DIR "${CMAKE_CURRENT_SOURCE_DIR}/build/rust-target/release")
    set(RUST_BUILD_FLAG "--release")
endif()

# Platform-specific library naming
if(WIN32)
    set(RUST_LIB_NAME "moq_wrapper.dll")
    set(RUST_LIB_PATH "${RUST_TARGET_DIR}/deps/${RUST_LIB_NAME}")
elseif(APPLE)
    set(RUST_LIB_NAME "libmoq_wrapper.dylib")
    set(RUST_LIB_PATH "${RUST_TARGET_DIR}/deps/${RUST_LIB_NAME}")
else()
    set(RUST_LIB_NAME "libmoq_wrapper.so")
    set(RUST_LIB_PATH "${RUST_TARGET_DIR}/deps/${RUST_LIB_NAME}")
endif()

# Custom target to build the Rust library
add_custom_target(rust_lib
    COMMAND ${CARGO_EXECUTABLE} build ${RUST_BUILD_FLAG}
    WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
    COMMENT "Building Rust library"
    BYPRODUCTS ${RUST_LIB_PATH}
)

# Create the C++ wrapper library
add_library(moq-cpp SHARED
    cpp/src/moq_wrapper.cpp
)

# Add dependency on Rust library
add_dependencies(moq-cpp rust_lib)

# Set include directories
target_include_directories(moq-cpp PUBLIC
    $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/cpp/include>
    $<INSTALL_INTERFACE:include>
)

# Create imported target for Rust library properly
add_library(moq_rust SHARED IMPORTED)
set_target_properties(moq_rust PROPERTIES
    IMPORTED_LOCATION ${RUST_LIB_PATH}
    IMPORTED_NO_SONAME TRUE
)
add_dependencies(moq_rust rust_lib)

# Link using the imported target instead of absolute path
target_link_libraries(moq-cpp PRIVATE moq_rust)

# Fix the library reference to use @rpath on macOS
if(APPLE)
    add_custom_command(TARGET moq-cpp POST_BUILD
        COMMAND ${CMAKE_INSTALL_NAME_TOOL} -change 
        "${RUST_LIB_PATH}" 
        "@rpath/libmoq_wrapper.dylib" 
        "$<TARGET_FILE:moq-cpp>"
        COMMENT "Fixing Rust library path to use @rpath"
    )
endif()

# Platform-specific linking
if(APPLE)
    target_link_libraries(moq-cpp PRIVATE "-framework CoreFoundation" "-framework Security")
elseif(UNIX)
    target_link_libraries(moq-cpp PRIVATE pthread dl)
elseif(WIN32)
    target_link_libraries(moq-cpp PRIVATE ws2_32 userenv bcrypt)
endif()

# Set RPATH for the library to find dependencies
if(APPLE)
    set_target_properties(moq-cpp PROPERTIES
        INSTALL_RPATH "@loader_path"
        BUILD_RPATH "${RUST_TARGET_DIR}"
        INSTALL_RPATH_USE_LINK_PATH TRUE
    )
elseif(UNIX)
    set_target_properties(moq-cpp PROPERTIES
        INSTALL_RPATH "$ORIGIN"
        BUILD_RPATH "${RUST_TARGET_DIR}"
        INSTALL_RPATH_USE_LINK_PATH TRUE
    )
endif()

# Set library properties
set_target_properties(moq-cpp PROPERTIES
    VERSION ${PROJECT_VERSION}
    SOVERSION 0
    CXX_STANDARD 17
    CXX_STANDARD_REQUIRED ON
)

# Google C++ Style Guide compliance
target_compile_options(moq-cpp PRIVATE
    $<$<CXX_COMPILER_ID:GNU>:-Wall -Wextra -Wpedantic>
    $<$<CXX_COMPILER_ID:Clang>:-Wall -Wextra -Wpedantic>
    $<$<CXX_COMPILER_ID:MSVC>:/W4>
)

# Install targets
install(TARGETS moq-cpp
    EXPORT moq-cpp-targets
    LIBRARY DESTINATION lib
    ARCHIVE DESTINATION lib
    RUNTIME DESTINATION bin
)

install(DIRECTORY cpp/include/ DESTINATION include)

# Install the Rust library alongside the C++ library
install(FILES ${RUST_LIB_PATH} DESTINATION lib)

# Export package
include(CMakePackageConfigHelpers)

configure_package_config_file(
    "moq-cpp-config.cmake.in"
    "moq-cpp-config.cmake"
    INSTALL_DESTINATION lib/cmake/moq-cpp
)

write_basic_package_version_file(
    "moq-cpp-config-version.cmake"
    VERSION ${PROJECT_VERSION}
    COMPATIBILITY SameMajorVersion
)

install(EXPORT moq-cpp-targets
    FILE moq-cpp-targets.cmake
    DESTINATION lib/cmake/moq-cpp
)

install(FILES
    "${CMAKE_CURRENT_BINARY_DIR}/moq-cpp-config.cmake"
    "${CMAKE_CURRENT_BINARY_DIR}/moq-cpp-config-version.cmake"
    DESTINATION lib/cmake/moq-cpp
)

# Copy Rust library to output directory for runtime
add_custom_command(TARGET moq-cpp POST_BUILD
    COMMAND ${CMAKE_COMMAND} -E copy_if_different
        ${RUST_LIB_PATH}
        $<TARGET_FILE_DIR:moq-cpp>
    COMMENT "Copying Rust library to output directory"
)