#include "moq-mgr/producer.h"

#include <chrono>
#include <stdexcept>

#include <moq/moq.h>

namespace moq_mgr {

Producer::Producer(size_t producer_id, const std::string &broadcast_id,
                   const BroadcastConfig& broadcast,
                   std::shared_ptr<moq::Session> moq_session)
    : producer_id_(producer_id)
    , broadcast_id_(broadcast_id)
    , broadcast_(broadcast)
    , moq_session_(moq_session)
    , start_time_(std::chrono::system_clock::now()) {
  if (!moq_session_) {
    throw std::invalid_argument("MoQ session cannot be null");
  }
}

Producer::~Producer() {
  stop();
}

void Producer::start() {
  std::unique_lock<std::mutex> lock(mutex_);
  
  if (running_.load()) {
    return;
  }
  
  running_.store(true);

  try {
    worker_thread_ = std::thread(&Producer::producer_loop, this);
  } catch (const std::exception& e) {
    running_.store(false);
  }
}

void Producer::stop() {
  {
    std::unique_lock<std::mutex> lock(mutex_);
    running_.store(false);
    condition_.notify_all();
  }
  
  if (worker_thread_.joinable()) {
    worker_thread_.join();
  }
}

bool Producer::is_running() const {
  return running_.load();
}

void Producer::producer_loop() {
  bool moq_setup = false;
  auto last_retry_time = std::chrono::steady_clock::now();
  const auto retry_interval = std::chrono::seconds(5);
  
  while (running_.load()) {
    try {
      // Try to set up MoQ producer if not yet established
      if (!moq_setup) {
        auto now = std::chrono::steady_clock::now();
        
        if (now - last_retry_time >= retry_interval) {
          if (setup_moq_producer()) {
            moq_setup = true;
            published_.store(true);
          } else {
            last_retry_time = now;
          }
        }
        
        if (!moq_setup) {
          std::unique_lock<std::mutex> lock(mutex_);
          condition_.wait_for(lock, std::chrono::seconds(1), [this] {
            return !running_.load();
          });
          continue;
        }
      }
      
      // Wait for events or shutdown signal
      std::unique_lock<std::mutex> lock(mutex_);
      condition_.wait(lock, [this] {
        return !running_.load();
      });
      
    } catch (const std::exception& e) {
      moq_setup = false;
      published_.store(false);
      moq_track_producer_.reset();
      moq_broadcast_producer_.reset();
      last_retry_time = std::chrono::steady_clock::now();
      
      if (!running_.load()) break;
      
      // Brief pause before retrying
      std::this_thread::sleep_for(std::chrono::milliseconds(100));
    }
  }
}

bool Producer::setup_moq_producer() {
  try {
    // Create a BroadcastProducer
    moq_broadcast_producer_ = std::make_shared<moq::BroadcastProducer>();
    if (!moq_broadcast_producer_) {
      return false;
    }
    
    // Create a TrackProducer for the specific track
    moq::Track track;
    track.name = broadcast_.moq_track_name;
    track.priority = 0;
    
    moq_track_producer_ = moq_broadcast_producer_->createTrack(track);
    if (!moq_track_producer_) {
      moq_broadcast_producer_.reset();
      return false;
    }
    
    // Get the consumable version and publish to the session
    auto consumable = moq_broadcast_producer_->getConsumable();
    if (!consumable) {
      moq_track_producer_.reset();
      moq_broadcast_producer_.reset();
      return false;
    }

    // Publish the broadcast to the MoQ session
    if (!moq_session_->publish(broadcast_id_, consumable)) {
      moq_track_producer_.reset();
      moq_broadcast_producer_.reset();
      return false;
    }
    
    return true;
    
  } catch (const std::exception& e) {
    moq_track_producer_.reset();
    moq_broadcast_producer_.reset();
    return false;
  }
}

}  // namespace moq_mgr