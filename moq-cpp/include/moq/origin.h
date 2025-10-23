#pragma once

#include <memory>
#include <string>
#include <optional>

namespace moq {

/// Structure representing an announced broadcast
struct Announce {
    /// The path of the announced broadcast
    std::string path;
    /// Whether the broadcast is active (true) or ended (false)  
    bool active;
};

/// OriginConsumer - handles announcements of available broadcasts
class OriginConsumer {
public:
    /// Destructor
    ~OriginConsumer();

    /// Get the next announced broadcast (blocking)
    /// This method blocks until the next broadcast announcement is available.
    /// @return Optional Announce on success, nullopt if stream ended or session closed
    std::optional<Announce> announced();

    /// Try to get the next announced broadcast (non-blocking)
    /// This method returns immediately without blocking.
    /// @return Optional Announce if available, nullopt if no announcement is available
    std::optional<Announce> tryAnnounced();

private:
    friend class Session;  // Allow Session to create OriginConsumer instances
    OriginConsumer(void* handle);
    void* handle_;
};

} // namespace moq