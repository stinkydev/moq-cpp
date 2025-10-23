#include "moq/session.h"
#include "moq/broadcast.h"
#include "moq/origin.h"

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
    if (!handle_) {
        return false;
    }
    return moq_session_is_connected(static_cast<const MoqSession*>(handle_));
}

// Check if session is still alive
bool Session::isAlive() const {
    if (!handle_) {
        return false;
    }
    return moq_session_is_alive(static_cast<const MoqSession*>(handle_));
}

// Close the session
void Session::close() {
    if (handle_) {
        moq_session_close(static_cast<MoqSession*>(handle_));
        moq_session_free(static_cast<MoqSession*>(handle_));
        handle_ = nullptr;
    }
}

// Publish a broadcast
bool Session::publish(const std::string& broadcast_name, std::shared_ptr<BroadcastProducer> producer) {
    if (!handle_ || !producer) {
        return false;
    }
    
    MoqResult result = moq_session_publish(
        static_cast<MoqSession*>(handle_),
        broadcast_name.c_str(),
        static_cast<MoqBroadcastProducer*>(producer->handle_)
    );
    
    return result == MoqResult::Success;
}

// Consume a broadcast
std::unique_ptr<BroadcastConsumer> Session::consume(const std::string& broadcast_name) {
    if (!handle_) {
        return nullptr;
    }
    
    MoqBroadcastConsumer* consumer = nullptr;
    MoqResult result = moq_session_consume(
        static_cast<MoqSession*>(handle_),
        broadcast_name.c_str(),
        &consumer
    );
    
    if (result == MoqResult::Success && consumer) {
        return std::unique_ptr<BroadcastConsumer>(new BroadcastConsumer(consumer));
    }
    
    return nullptr;
}

// Get the origin consumer for announcements  
std::unique_ptr<OriginConsumer> Session::getOriginConsumer() {
    if (!handle_) {
        return nullptr;
    }
    
    MoqOriginConsumer* origin_consumer = nullptr;
    MoqResult result = moq_session_get_origin_consumer(
        static_cast<MoqSession*>(handle_),
        &origin_consumer
    );
    
    if (result == MoqResult::Success && origin_consumer) {
        return std::unique_ptr<OriginConsumer>(new OriginConsumer(origin_consumer));
    }
    
    return nullptr;
}

} // namespace moq
