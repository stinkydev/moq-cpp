#pragma once

#include <atomic>
#include <chrono>
#include <condition_variable>
#include <memory>
#include <mutex>
#include <set>
#include <string>
#include <thread>
#include <vector>
#include <functional>

// Forward declarations
namespace moq {
class Session;
class BroadcastConsumer;
class TrackConsumer;
}  // namespace moq

namespace moq_mgr {

// Forward declaration
class Session;

/**
 * @brief Consumer class for handling a single MoQ subscription
 *
 * Consumer handles receiving data from a specific MoQ track via blocking reads
 * and forwarding it to Cachearoo as either object updates or signal events.
 * Each consumer is dedicated to one subscription.
 */
class Consumer {
 public:
  struct SubscriptionConfig {
    std::string moq_track_name;
    std::function<void(uint8_t* data, size_t size)> data_callback;
  };

  explicit Consumer(
      size_t consumer_id, const std::string& broadcast_id,
      const SubscriptionConfig& subscription,
      std::shared_ptr<moq::Session> moq_session);
  ~Consumer();

  /**
   * @brief Start the consumer
   */
  void start();

  /**
   * @brief Stop the consumer
   */
  void stop();

  /**
   * @brief Check if the consumer is running
   * @return true if running, false otherwise
   */
  bool is_running() const;

  /**
   * @brief Get consumer ID
   * @return Consumer ID
   */
  size_t get_consumer_id() const { return consumer_id_; }

 private:
  size_t consumer_id_;
  std::string broadcast_id_;
  SubscriptionConfig subscription_;
  std::shared_ptr<moq::Session> moq_session_;

  std::unique_ptr<moq::BroadcastConsumer> moq_consumer_;
  std::unique_ptr<moq::TrackConsumer> moq_track_consumer_;

  std::atomic<bool> running_{false};
  std::atomic<bool> subscribed_{false};
  std::atomic<uint64_t> bytes_received_{0};
  std::atomic<uint64_t> messages_received_{0};
  mutable std::mutex mutex_;
  std::condition_variable condition_;
  std::thread worker_thread_;

  std::chrono::system_clock::time_point last_data_time_;
  std::chrono::system_clock::time_point start_time_;

  /**
   * @brief Main consumer loop that waits for MoQ data via blocking reads
   */
  void consumer_loop();

  /**
   * @brief Establish subscription to the MoQ track
   * @return true if subscription was established successfully
   */
  bool establish_subscription();

  /**
   * @brief Handle received MoQ data and forward to Cachearoo
   * @param data Raw data received from MoQ
   */
  void handle_moq_data(const std::vector<uint8_t>& data);
};

}  // namespace moq_cro_bridge