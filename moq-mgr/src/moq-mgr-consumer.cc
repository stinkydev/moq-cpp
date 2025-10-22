#include "moq-mgr/consumer.h"

#include <chrono>
#include <stdexcept>

#include <moq/moq.h>

namespace moq_mgr {

Consumer::Consumer(size_t consumer_id, const std::string &broadcast_id,
                   const SubscriptionConfig& subscription,
                   std::shared_ptr<moq::Session> moq_session)
    : consumer_id_(consumer_id)
    , broadcast_id_(broadcast_id)
    , subscription_(subscription)
    , moq_session_(moq_session)
    , start_time_(std::chrono::system_clock::now()) {
  if (!moq_session_) {
    throw std::invalid_argument("MoQ session cannot be null");
  }
}

Consumer::~Consumer() {
  stop();
}

void Consumer::start() {
  std::unique_lock<std::mutex> lock(mutex_);
  
  if (running_.load()) {
    return;
  }
  
  running_.store(true);

  try {
    // Start the worker thread - it will handle subscription establishment with retry logic
    worker_thread_ = std::thread(&Consumer::consumer_loop, this);
  } catch (const std::exception& e) {
    running_.store(false);
  }
}

void Consumer::stop() {
  {
    std::unique_lock<std::mutex> lock(mutex_);
    running_.store(false);
    condition_.notify_all();
  }
  
  if (worker_thread_.joinable()) {
    worker_thread_.join();
  }
}

bool Consumer::is_running() const {
  return running_.load();
}

void Consumer::consumer_loop() {
  bool subscription_established = false;
  auto last_retry_time = std::chrono::steady_clock::now();
  const auto retry_interval = std::chrono::seconds(5);
  
  while (running_.load()) {
    try {
      // Try to establish subscription if not yet connected
      if (!subscription_established) {
        auto now = std::chrono::steady_clock::now();
        
        // Only retry after the retry interval has elapsed
        if (now - last_retry_time >= retry_interval) {
          if (establish_subscription()) {
            subscription_established = true;
            subscribed_.store(true);
          } else {
            // Subscription failed, will retry after interval
            last_retry_time = now;
          }
        }
        
        // If still not subscribed, wait before next retry attempt
        if (!subscription_established) {
          std::unique_lock<std::mutex> lock(mutex_);
          condition_.wait_for(lock, std::chrono::seconds(1), [this] {
            return !running_.load();
          });
          continue;
        }
      }
      
      // Read MoQ data from the subscribed track
      if (subscription_established && moq_track_consumer_) {
        try {
          // Get the next group from the track (blocking call)
          auto group_future = moq_track_consumer_->nextGroup();
          auto group_consumer = group_future.get();  // Block until next group arrives
          
          if (!group_consumer) {
            // No more groups (stream ended or closed)
            subscription_established = false;
            subscribed_.store(false);
            moq_track_consumer_.reset();
            moq_consumer_.reset();
            last_retry_time = std::chrono::steady_clock::now();
            continue;
          }
          
          // Read all frames from this group
          while (running_.load()) {
            auto frame_future = group_consumer->readFrame();
            auto frame_status = frame_future.wait_for(std::chrono::milliseconds(100));
            
            if (frame_status == std::future_status::ready) {
              auto frame_data = frame_future.get();
              
              if (!frame_data.has_value()) {
                break;
              }
              
              // Handle the received frame data
              handle_moq_data(frame_data.value());
            } else {
              // Timeout waiting for frame, check if we should continue
              if (!running_.load()) break;
            }
          }
          
          // After finishing this group, loop will continue to get next group
          
        } catch (const std::exception& e) {
          subscription_established = false;
          subscribed_.store(false);
          moq_track_consumer_.reset();
          moq_consumer_.reset();
          last_retry_time = std::chrono::steady_clock::now();
          
          std::this_thread::sleep_for(std::chrono::milliseconds(100));
        }
      }
      
    } catch (const std::exception& e) {
      // On error, reset subscription and retry
      subscription_established = false;
      subscribed_.store(false);
      moq_track_consumer_.reset();
      moq_consumer_.reset();
      last_retry_time = std::chrono::steady_clock::now();
      
      if (!running_.load()) break;
      
      // Brief pause before retrying
      std::this_thread::sleep_for(std::chrono::milliseconds(100));
    }
  }
}

bool Consumer::establish_subscription() {
  try {
    if (moq_track_consumer_) {
      moq_consumer_.reset();
    }
    moq_consumer_ = moq_session_->consume(broadcast_id_);
    if (!moq_consumer_) {
      // Silently return false - producer may not be available yet
      return false;
    }

    moq_track_consumer_ = moq_consumer_->subscribeTrack(moq::Track{subscription_.moq_track_name});
    if (!moq_track_consumer_) {
      // Silently return false - track may not be available yet
      moq_consumer_.reset();
      return false;
    }

    return true;
    
  } catch (const std::exception& e) {
    moq_track_consumer_.reset();
    moq_consumer_.reset();
    return false;
  }
}

void Consumer::handle_moq_data(const std::vector<uint8_t>& data) {
  // Update statistics
  bytes_received_.fetch_add(data.size());
  messages_received_.fetch_add(1);
  last_data_time_ = std::chrono::system_clock::now();

  if (subscription_.data_callback) {
    subscription_.data_callback(const_cast<uint8_t*>(data.data()), data.size());
  }
}


}  // namespace moq_cro_bridge