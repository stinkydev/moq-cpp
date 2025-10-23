#include "moq/origin.h"
#include "moq_ffi.h"
#include <optional>

namespace moq {

// OriginConsumer implementation
OriginConsumer::OriginConsumer(void* handle) : handle_(handle) {
    // Test the simple function to verify linking works
    // int test_result = moq_test_simple_function(42);
    // (void)test_result; // Suppress unused variable warning
}

OriginConsumer::~OriginConsumer() {
    // Temporarily disabled for linking test
    /*
    if (handle_) {
        moq_origin_consumer_free(static_cast<MoqOriginConsumer*>(handle_));
    }
    */
}

std::optional<Announce> OriginConsumer::announced() {
    if (!handle_) {
        return std::nullopt;
    }
    
    MoqAnnounce announce;
    MoqAnnounceResult result = moq_origin_consumer_announced(
        static_cast<MoqOriginConsumer*>(handle_),
        &announce
    );
    
    if (result == MoqAnnounceResult::AnnounceSuccess) {
        Announce announcement;
        announcement.path = std::string(announce.path);
        announcement.active = announce.active;
        
        // Free the announce structure's path
        moq_announce_free(&announce);
        
        return announcement;
    }
    
    return std::nullopt;
}

std::optional<Announce> OriginConsumer::tryAnnounced() {
    if (!handle_) {
        return std::nullopt;
    }
    
    MoqAnnounce announce;
    MoqAnnounceResult result = moq_origin_consumer_try_announced(
        static_cast<MoqOriginConsumer*>(handle_),
        &announce
    );
    
    if (result == MoqAnnounceResult::AnnounceSuccess) {
        Announce announcement;
        announcement.path = std::string(announce.path);
        announcement.active = announce.active;
        
        // Free the announce structure's path
        moq_announce_free(&announce);
        
        return announcement;
    }
    
    return std::nullopt;
}

} // namespace moq