#pragma once

#include <atomic>
#include <chrono>
#include <condition_variable>
#include <map>
#include <memory>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

// Forward declarations
namespace moq {
class Session;
class BroadcastProducer;
class TrackProducer;
class GroupProducer;
}  // namespace moq

namespace moq_mgr {

// Forward declaration
class Session;

/**
 * @brief Producer class for handling a single MoQ broadcast
 *
 * Producer listens for Cachearoo object changes via event listeners and
 * publishes updates to its corresponding MoQ track. Each producer is dedicated
 * to one broadcast configuration and uses event-driven architecture.
 */
class Producer {
 public:
  struct BroadcastConfig {
    std::string moq_track_name;
  };

  explicit Producer(
      size_t producer_id, const std::string& broadcast_id,
      const BroadcastConfig& broadcast,
      std::shared_ptr<moq::Session> moq_session);
  ~Producer();

  /**
   * @brief Start the producer
   */
  void start();

  /**
   * @brief Stop the producer
   */
  void stop();

  /**
   * @brief Check if the producer is running
   * @return true if running, false otherwise
   */
  bool is_running() const;

  /**
   * @brief Get producer ID
   * @return Producer ID
   */
  size_t get_producer_id() const { return producer_id_; }

 private:
  size_t producer_id_;
  std::string broadcast_id_;
  BroadcastConfig broadcast_;
  std::shared_ptr<moq::Session> moq_session_;

  std::shared_ptr<moq::BroadcastProducer> moq_broadcast_producer_;
  std::unique_ptr<moq::TrackProducer> moq_track_producer_;
  std::unique_ptr<moq::GroupProducer> moq_group_producer_;

  std::atomic<bool> running_{false};
  std::atomic<bool> published_{false};
  std::atomic<uint64_t> bytes_sent_{0};
  std::atomic<uint64_t> messages_sent_{0};
  // start with random sequence to avoid collisions on restart
  std::atomic<uint64_t> group_sequence_{
      static_cast<uint64_t>(std::rand() % 1000000)};
  mutable std::mutex mutex_;
  std::condition_variable condition_;
  std::thread worker_thread_;

  std::chrono::system_clock::time_point last_data_time_;
  std::chrono::system_clock::time_point start_time_;

  /**
   * @brief Main producer loop that waits for Cachearoo events
   */
  void producer_loop();

  /**
   * @brief Setup MoQ track producer
   * @return true if setup was successful
   */
  bool setup_moq_producer();
};

}  // namespace moq_cro_bridge