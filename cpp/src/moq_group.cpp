#include "moq/group.h"
#include <thread>
#include <chrono>

// Include the generated C header
extern "C" {
    #include "moq_ffi.h"
}

namespace moq {

// GroupProducer implementation
GroupProducer::GroupProducer(void* handle) : handle_(handle), finished_(false) {}

GroupProducer::~GroupProducer() {
    if (!finished_) {
        finish();
    }
    
    if (handle_) {
        moq_group_producer_free(static_cast<MoqGroupProducer*>(handle_));
    }
}

bool GroupProducer::writeFrame(const std::vector<uint8_t>& data) {
    if (finished_ || !handle_) {
        return false;
    }
    
    MoqResult result = moq_group_producer_write_frame(
        static_cast<MoqGroupProducer*>(handle_),
        data.empty() ? nullptr : data.data(),
        data.size()
    );
    
    return result == MoqResult::Success;
}

bool GroupProducer::writeFrame(const std::string& data) {
    if (finished_ || !handle_) {
        return false;
    }
    
    MoqResult result = moq_group_producer_write_frame(
        static_cast<MoqGroupProducer*>(handle_),
        reinterpret_cast<const uint8_t*>(data.c_str()),
        data.size()
    );
    
    return result == MoqResult::Success;
}

bool GroupProducer::writeFrame(const uint8_t* data, size_t size) {
    if (finished_ || !handle_ || !data) {
        return false;
    }
    
    MoqResult result = moq_group_producer_write_frame(
        static_cast<MoqGroupProducer*>(handle_),
        data,
        size
    );
    
    return result == MoqResult::Success;
}

void GroupProducer::finish() {
    if (!finished_) {
        finished_ = true;
        
        if (handle_) {
            moq_group_producer_finish(static_cast<MoqGroupProducer*>(handle_));
        }
    }
}

// GroupConsumer implementation
GroupConsumer::GroupConsumer(void* handle) : handle_(handle) {}

GroupConsumer::~GroupConsumer() {
    if (handle_) {
        moq_group_consumer_free(static_cast<MoqGroupConsumer*>(handle_));
    }
}

std::future<std::optional<std::vector<uint8_t>>> GroupConsumer::readFrame() {
    return std::async(std::launch::async, [this]() -> std::optional<std::vector<uint8_t>> {
        if (!handle_) {
            return std::nullopt;
        }
        
        uint8_t* data = nullptr;
        size_t data_len = 0;
        
        MoqResult result = moq_group_consumer_read_frame(
            static_cast<MoqGroupConsumer*>(handle_),
            &data,
            &data_len
        );
        
        if (result == MoqResult::Success && data && data_len > 0) {
            std::vector<uint8_t> frame(data, data + data_len);
            // Free the data allocated by FFI
            moq_free(data);
            return frame;
        }
        
        return std::nullopt;
    });
}

} // namespace moq
