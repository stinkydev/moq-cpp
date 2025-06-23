#pragma once

#include <memory>
#include <string>
#include <functional>
#include <vector>

namespace moq {

/// Callback function type for receiving track data
using DataCallback = std::function<void(const std::string& track_name, const std::vector<uint8_t>& data)>;

/// MOQ Track class - represents a published or subscribed track
class Track {
public:
    /// Destructor
    ~Track();

    /// Send data on a published track
    /// @param data The data to send
    /// @return true on success, false on failure
    bool sendData(const std::vector<uint8_t>& data);

    /// Send data on a published track
    /// @param data The data to send
    /// @param size Size of the data
    /// @return true on success, false on failure
    bool sendData(const uint8_t* data, size_t size);

    /// Send string data on a published track
    /// @param data The string data to send
    /// @return true on success, false on failure
    bool sendData(const std::string& data);

    /// Get the track name
    /// @return The name of the track
    const std::string& getName() const;

    /// Check if this is a publisher track
    /// @return true if this track can publish data
    bool isPublisher() const;

private:
    friend class Session;
    Track(void* handle, const std::string& name, bool is_publisher);
    void* handle_;
    std::string name_;
    bool is_publisher_;
};

} // namespace moq
