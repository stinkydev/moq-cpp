#pragma once

#include <memory>
#include <functional>
#include <vector>
#include <string>

namespace moq {

/// Forward declarations
class Track;

/// Callback function type for receiving track data
using DataCallback = std::function<void(const std::string& track_name, const std::vector<uint8_t>& data)>;

/// MOQ Session class - represents a connection to a MOQ server
class Session {
public:
    /// Destructor
    ~Session();

    /// Check if the session is connected
    /// @return true if connected, false otherwise
    bool isConnected() const;

    /// Close the session
    void close();

    /// Publish a track
    /// @param track_name Name of the track to publish
    /// @return Unique pointer to a Track on success, nullptr on failure
    std::unique_ptr<Track> publishTrack(const std::string& track_name);

    /// Subscribe to a track
    /// @param track_name Name of the track to subscribe to
    /// @param callback Callback function for received data
    /// @return Unique pointer to a Track on success, nullptr on failure
    std::unique_ptr<Track> subscribeTrack(const std::string& track_name, DataCallback callback);

private:
    friend class Client;
    Session(void* handle);
    void* handle_;
};

} // namespace moq
