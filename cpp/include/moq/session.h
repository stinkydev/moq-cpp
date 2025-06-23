#pragma once

#include <memory>

namespace moq {

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

private:
    friend class Client;
    Session(void* handle);
    void* handle_;
};

} // namespace moq
