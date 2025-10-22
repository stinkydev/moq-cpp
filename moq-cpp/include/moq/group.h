#pragma once

#include <memory>
#include <string>
#include <vector>
#include <optional>
#include <future>

namespace moq {

/// GroupProducer - publishes frame data within a group
class GroupProducer {
public:
    /// Destructor
    ~GroupProducer();

    /// Write a frame of data to the group
    /// @param data Frame data as a vector of bytes
    /// @return true on success, false on failure
    bool writeFrame(const std::vector<uint8_t>& data);

    /// Write a frame of data to the group
    /// @param data Frame data as a string
    /// @return true on success, false on failure
    bool writeFrame(const std::string& data);

    /// Write a frame of data to the group
    /// @param data Pointer to frame data
    /// @param size Size of the data
    /// @return true on success, false on failure
    bool writeFrame(const uint8_t* data, size_t size);

    /// Finish the group (no more frames will be written)
    void finish();

private:
    friend class TrackProducer;
    GroupProducer(void* handle);
    void* handle_;
    bool finished_ = false;
};

/// GroupConsumer - consumes frame data within a group
class GroupConsumer {
public:
    /// Destructor
    ~GroupConsumer();

    /// Read the next frame from the group
    /// @return Future that resolves to frame data, empty optional when no more frames
    std::future<std::optional<std::vector<uint8_t>>> readFrame();

private:
    friend class TrackConsumer;
    GroupConsumer(void* handle);
    void* handle_;
};

} // namespace moq
