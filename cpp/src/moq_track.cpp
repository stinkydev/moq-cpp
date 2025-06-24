#include "moq/track.h"
#include "moq/group.h"
#include <thread>
#include <chrono>

// Include the generated C header
extern "C" {
    #include "moq_ffi.h"
}

namespace moq {

// TrackProducer implementation
TrackProducer::TrackProducer(void* handle) : handle_(handle) {}

TrackProducer::~TrackProducer() {
    if (handle_) {
        moq_track_producer_free(static_cast<MoqTrackProducer*>(handle_));
    }
}

std::unique_ptr<GroupProducer> TrackProducer::createGroup(uint64_t sequence_number) {
    if (!handle_) {
        return nullptr;
    }
    
    MoqGroupProducer* group_handle = nullptr;
    MoqResult result = moq_track_producer_create_group(
        static_cast<MoqTrackProducer*>(handle_),
        sequence_number,
        &group_handle
    );
    
    if (result != MoqResult::Success || !group_handle) {
        return nullptr;
    }
    
    return std::unique_ptr<GroupProducer>(new GroupProducer(group_handle));
}

// TrackConsumer implementation
TrackConsumer::TrackConsumer(void* handle) : handle_(handle) {}

TrackConsumer::~TrackConsumer() {
    if (handle_) {
        moq_track_consumer_free(static_cast<MoqTrackConsumer*>(handle_));
    }
}

std::future<std::unique_ptr<GroupConsumer>> TrackConsumer::nextGroup() {
    return std::async(std::launch::async, [this]() -> std::unique_ptr<GroupConsumer> {
        if (!handle_) {
            return nullptr;
        }
        
        MoqGroupConsumer* group_handle = nullptr;
        MoqResult result = moq_track_consumer_next_group(
            static_cast<MoqTrackConsumer*>(handle_),
            &group_handle
        );
        
        if (result != MoqResult::Success || !group_handle) {
            return nullptr;
        }
        
        return std::unique_ptr<GroupConsumer>(new GroupConsumer(group_handle));
    });
}

} // namespace moq
