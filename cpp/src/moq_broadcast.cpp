#include "moq/broadcast.h"
#include "moq/track.h"

// Include the generated C header
extern "C" {
    #include "moq_ffi.h"
}

namespace moq {

// BroadcastProducer implementation
BroadcastProducer::BroadcastProducer() : handle_(nullptr) {
    MoqBroadcastProducer* producer = nullptr;
    MoqResult result = moq_broadcast_producer_new(&producer);
    if (result == MoqResult::Success && producer) {
        handle_ = producer;
    }
}

BroadcastProducer::~BroadcastProducer() {
    if (handle_) {
        moq_broadcast_producer_free(static_cast<MoqBroadcastProducer*>(handle_));
    }
}

std::unique_ptr<TrackProducer> BroadcastProducer::createTrack(const Track& track) {
    if (!handle_) {
        return nullptr;
    }

    MoqTrack ffi_track;
    ffi_track.name = track.name.c_str();
    ffi_track.priority = track.priority;

    MoqTrackProducer* track_producer = nullptr;
    MoqResult result = moq_broadcast_producer_create_track(
        static_cast<MoqBroadcastProducer*>(handle_),
        &ffi_track,
        &track_producer
    );

    if (result == MoqResult::Success && track_producer) {
        return std::unique_ptr<TrackProducer>(new TrackProducer(track_producer));
    }

    return nullptr;
}

std::shared_ptr<BroadcastProducer> BroadcastProducer::getConsumable() {
    // This should return a copy or a weak reference, not this directly
    // For now, we'll create a new shared_ptr that doesn't delete on destruction
    return std::shared_ptr<BroadcastProducer>(this, [](BroadcastProducer*){});
}

// BroadcastConsumer implementation
BroadcastConsumer::BroadcastConsumer(void* handle) : handle_(handle) {}

BroadcastConsumer::~BroadcastConsumer() {
    if (handle_) {
        moq_broadcast_consumer_free(static_cast<MoqBroadcastConsumer*>(handle_));
    }
}

std::unique_ptr<TrackConsumer> BroadcastConsumer::subscribeTrack(const Track& track) {
    if (!handle_) {
        return nullptr;
    }
    
    MoqTrack ffi_track;
    ffi_track.name = track.name.c_str();
    ffi_track.priority = track.priority;

    MoqTrackConsumer* track_consumer = nullptr;
    MoqResult result = moq_broadcast_consumer_subscribe_track(
        static_cast<MoqBroadcastConsumer*>(handle_),
        &ffi_track,
        &track_consumer
    );

    if (result == MoqResult::Success && track_consumer) {
        return std::unique_ptr<TrackConsumer>(new TrackConsumer(track_consumer));
    }

    return nullptr;
}

} // namespace moq
