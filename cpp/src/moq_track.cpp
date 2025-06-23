#include "moq/track.h"

// Include the generated C header
extern "C" {
    typedef struct MoqTrack MoqTrack;
    
    typedef enum {
        MoqResult_Success = 0,
        MoqResult_InvalidArgument = 1,
        MoqResult_NetworkError = 2,
        MoqResult_TlsError = 3,
        MoqResult_DnsError = 4,
        MoqResult_GeneralError = 5,
    } MoqResult;
    
    void moq_track_free(struct MoqTrack *track);
    MoqResult moq_track_send_data(struct MoqTrack *track,
                                  const uint8_t *data,
                                  uintptr_t data_len);
}

namespace moq {

// Private constructor
Track::Track(void* handle, const std::string& name, bool is_publisher) 
    : handle_(handle), name_(name), is_publisher_(is_publisher) {}

// Destructor
Track::~Track() {
    if (handle_) {
        moq_track_free(static_cast<MoqTrack*>(handle_));
    }
}

// Send data (vector)
bool Track::sendData(const std::vector<uint8_t>& data) {
    if (!handle_ || !is_publisher_ || data.empty()) {
        return false;
    }

    MoqResult result = moq_track_send_data(
        static_cast<MoqTrack*>(handle_),
        data.data(),
        data.size()
    );

    return result == MoqResult_Success;
}

// Send data (raw pointer)
bool Track::sendData(const uint8_t* data, size_t size) {
    if (!handle_ || !is_publisher_ || !data || size == 0) {
        return false;
    }

    MoqResult result = moq_track_send_data(
        static_cast<MoqTrack*>(handle_),
        data,
        size
    );

    return result == MoqResult_Success;
}

// Send string data
bool Track::sendData(const std::string& data) {
    if (!handle_ || !is_publisher_ || data.empty()) {
        return false;
    }

    MoqResult result = moq_track_send_data(
        static_cast<MoqTrack*>(handle_),
        reinterpret_cast<const uint8_t*>(data.c_str()),
        data.length()
    );

    return result == MoqResult_Success;
}

// Get track name
const std::string& Track::getName() const {
    return name_;
}

// Check if publisher
bool Track::isPublisher() const {
    return is_publisher_;
}

} // namespace moq
