#include "moq/session.h"

// Include the generated C header
extern "C" {
    #include "moq_ffi.h"
}

namespace moq {

// Private constructor
Session::Session(void* handle) : handle_(handle) {}

// Destructor
Session::~Session() {
    if (handle_) {
        moq_session_free(static_cast<MoqSession*>(handle_));
    }
}

// Check if connected
bool Session::isConnected() const {
    // For now, assume connected if we have a handle
    // In a real implementation, you'd want to check the actual connection state
    return handle_ != nullptr;
}

// Close the session
void Session::close() {
    if (handle_) {
        moq_session_free(static_cast<MoqSession*>(handle_));
        handle_ = nullptr;
    }
}

} // namespace moq
