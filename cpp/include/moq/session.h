#pragma once

#include <memory>
#include <functional>
#include <vector>
#include <string>

namespace moq {

/// Forward declarations
class BroadcastProducer;
class BroadcastConsumer;

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

    /// Publish a broadcast (equivalent to session.publish in Rust)
    /// @param broadcast_name Name of the broadcast to publish
    /// @param producer The broadcast producer to use for publishing
    /// @return true on success, false on failure
    bool publish(const std::string& broadcast_name, std::shared_ptr<BroadcastProducer> producer);

    /// Consume a broadcast (equivalent to session.consume in Rust)
    /// @param broadcast_name Name of the broadcast to consume
    /// @return Unique pointer to a BroadcastConsumer on success, nullptr on failure
    std::unique_ptr<BroadcastConsumer> consume(const std::string& broadcast_name);

private:
    friend class Client;
    Session(void* handle);
    void* handle_;
};

} // namespace moq
