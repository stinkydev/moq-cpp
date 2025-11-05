cmake_minimum_required(VERSION 3.16)
project(moq-cpp-examples VERSION 1.0.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Find the moq-cpp package
# This demonstrates how external projects would use the library
find_package(moq-cpp REQUIRED)

# Clock publisher example
add_executable(clock_publisher_example
    clock_publisher.cpp
)
target_link_libraries(clock_publisher_example PRIVATE moq-cpp::moq-cpp)

# Clock subscriber example  
add_executable(clock_subscriber_example
    clock_subscriber.cpp
)
target_link_libraries(clock_subscriber_example PRIVATE moq-cpp::moq-cpp)

# Set RPATH for examples to find libraries
if(APPLE)
    set_target_properties(clock_publisher_example clock_subscriber_example PROPERTIES
        INSTALL_RPATH "@executable_path/../lib"
        BUILD_RPATH "${CMAKE_PREFIX_PATH}/lib"
    )
elseif(UNIX)
    set_target_properties(clock_publisher_example clock_subscriber_example PROPERTIES
        INSTALL_RPATH "$ORIGIN/../lib"
        BUILD_RPATH "${CMAKE_PREFIX_PATH}/lib"
    )
endif()

# Install examples
install(TARGETS clock_publisher_example clock_subscriber_example
    RUNTIME DESTINATION bin/examples
)