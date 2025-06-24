#pragma once

#include <memory>
#include <string>
#include <functional>
#include <vector>
#include <optional>
#include <future>

namespace moq {

/// Forward declarations
class GroupProducer;
class GroupConsumer;

/// TrackProducer - publishes data to a track in groups
class TrackProducer {
public:
    /// Destructor
    ~TrackProducer();

    /// Create a new group for publishing data
    /// @param sequence_number Sequence number for the group
    /// @return Unique pointer to GroupProducer on success, nullptr on failure
    std::unique_ptr<GroupProducer> createGroup(uint64_t sequence_number);

private:
    friend class BroadcastProducer;
    TrackProducer(void* handle);
    void* handle_;
};

/// TrackConsumer - consumes data from a track in groups
class TrackConsumer {
public:
    /// Destructor
    ~TrackConsumer();

    /// Get the next group of data from the track
    /// @return Future that resolves to GroupConsumer on success, nullptr when stream ends
    std::future<std::unique_ptr<GroupConsumer>> nextGroup();

private:
    friend class BroadcastConsumer;
    TrackConsumer(void* handle);
    void* handle_;
};

} // namespace moq
