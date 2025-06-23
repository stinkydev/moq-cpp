#include "moq/session.h"
#include "moq/track.h"
#include <functional>
#include <unordered_map>

// Include the generated C header
extern "C" {
    typedef struct MoqSession MoqSession;
    typedef struct MoqTrack MoqTrack;
    
    typedef enum {
        MoqResult_Success = 0,
        MoqResult_InvalidArgument = 1,
        MoqResult_NetworkError = 2,
        MoqResult_TlsError = 3,
        MoqResult_DnsError = 4,
        MoqResult_GeneralError = 5,
    } MoqResult;
    
    void moq_session_free(struct MoqSession *session);
    bool moq_session_is_connected(const struct MoqSession *session);
    MoqResult moq_session_close(struct MoqSession *session);
    
    MoqResult moq_session_publish_track(struct MoqSession *session,
                                        const char *track_name,
                                        struct MoqTrack **track_out);
    
    MoqResult moq_session_subscribe_track(struct MoqSession *session,
                                          const char *track_name,
                                          void (*callback)(const char*, const uint8_t*, uintptr_t, void*),
                                          void *user_data,
                                          struct MoqTrack **track_out);
}

// Global storage for callback mappings
static std::unordered_map<std::string, moq::DataCallback> g_callbacks;

// C callback wrapper
extern "C" void moq_data_callback_wrapper(
    const char* track_name,
    const uint8_t* data,
    size_t data_len,
    void* user_data
) {
    std::string name(track_name);
    auto it = g_callbacks.find(name);
    if (it != g_callbacks.end()) {
        std::vector<uint8_t> data_vec(data, data + data_len);
        it->second(name, data_vec);
    }
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

// Publish a track
std::unique_ptr<Track> Session::publishTrack(const std::string& track_name) {
    if (!handle_) {
        return nullptr;
    }

    MoqTrack* track_handle = nullptr;
    MoqResult result = moq_session_publish_track(
        static_cast<MoqSession*>(handle_),
        track_name.c_str(),
        &track_handle
    );

    if (result != MoqResult_Success || !track_handle) {
        return nullptr;
    }

    return std::unique_ptr<Track>(new Track(track_handle, track_name, true));
}

// Subscribe to a track
std::unique_ptr<Track> Session::subscribeTrack(const std::string& track_name, DataCallback callback) {
    if (!handle_) {
        return nullptr;
    }

    // Store the callback
    g_callbacks[track_name] = callback;

    MoqTrack* track_handle = nullptr;
    MoqResult result = moq_session_subscribe_track(
        static_cast<MoqSession*>(handle_),
        track_name.c_str(),
        moq_data_callback_wrapper,
        nullptr,
        &track_handle
    );

    if (result != MoqResult_Success || !track_handle) {
        g_callbacks.erase(track_name);
        return nullptr;
    }

    return std::unique_ptr<Track>(new Track(track_handle, track_name, false));
}

} // namespace moq
