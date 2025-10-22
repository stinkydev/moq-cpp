#pragma once

#include <memory>
#include <string>
#include <functional>
#include <vector>

namespace moq {

/// Forward declarations
class TrackProducer;
class TrackConsumer;

/// Track information structure
struct Track {
    std::string name;
    uint8_t priority = 0;
};

/// BroadcastProducer - manages publishing multiple tracks in a broadcast
class BroadcastProducer {
public:
    /// Constructor
    BroadcastProducer();

    /// Destructor
    ~BroadcastProducer();

    /// Create a track producer for the given track
    /// @param track Track information
    /// @return Unique pointer to TrackProducer on success, nullptr on failure
    std::unique_ptr<TrackProducer> createTrack(const Track& track);

    /// Get a consumable version of this producer for publishing
    /// @return Shared pointer to BroadcastProducer for consumption
    std::shared_ptr<BroadcastProducer> getConsumable();

private:
    friend class Session;  // Allow Session to access handle_
    void* handle_;
};

/// BroadcastConsumer - manages consuming multiple tracks from a broadcast
class BroadcastConsumer {
public:
    /// Destructor
    ~BroadcastConsumer();

    /// Subscribe to a specific track in the broadcast
    /// @param track Track information to subscribe to
    /// @return Unique pointer to TrackConsumer on success, nullptr on failure
    std::unique_ptr<TrackConsumer> subscribeTrack(const Track& track);

private:
    friend class Session;
    BroadcastConsumer(void* handle);
    void* handle_;
};

} // namespace moq
