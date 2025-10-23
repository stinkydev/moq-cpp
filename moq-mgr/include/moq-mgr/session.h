#pragma once

#include <atomic>
#include <chrono>
#include <condition_variable>
#include <functional>
#include <map>
#include <memory>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

#include "moq-mgr/consumer.h"
#include "moq-mgr/producer.h"

#include <moq/moq.h>

// Forward declarations
namespace moq {
class Client;
class Session;
class OriginConsumer;
}  // namespace moq

namespace moq_mgr {

class Session {
 public:
  struct SessionConfig {
    std::string moq_server;
    std::string moq_namespace;
    bool reconnect_on_failure{true};
  };

  explicit Session(const SessionConfig& config, moq::SessionMode mode);
  ~Session();

  bool start();
  void stop();
  bool is_running() const;

  void set_error_callback(std::function<void(const std::string&)> callback);
  void set_status_callback(std::function<void(const std::string&)> callback);
 protected:
  std::unique_ptr<moq::Client> moq_client_;
  std::shared_ptr<moq::Session> moq_session_;
  SessionConfig config_;
  std::atomic<bool> running_{false};
  mutable std::mutex mutex_;
  std::condition_variable condition_;

  std::thread session_thread_;

  std::function<void(const std::string&)> error_callback_;
  std::function<void(const std::string&)> status_callback_;
  void notify_error(const std::string& error);
  void notify_status(const std::string& status);

 protected:
  moq::SessionMode mode_;
  
  // Reconnection timing
  std::chrono::steady_clock::time_point last_reconnect_attempt_{};
  bool first_reconnect_attempt_{true};

  void session_loop();
  bool reconnect();

  virtual void stop_all_workers() = 0;
  virtual void start_all_workers() = 0;
  virtual void cleanup_connections() = 0;
};

class ProducerSession : public Session {
 public:
  explicit ProducerSession(const SessionConfig& config, std::vector<Producer::BroadcastConfig> broadcasts) : Session(config, moq::SessionMode::PublishOnly), broadcasts_(std::move(broadcasts)) {}

 private:
  std::vector<Producer::BroadcastConfig> broadcasts_;
  std::vector<std::unique_ptr<Producer>> producers_;

 protected:
  void start_all_workers() override;
  void stop_all_workers() override;
  void cleanup_connections() override;
};

class ConsumerSession : public Session {
 public:
  explicit ConsumerSession(const SessionConfig& config, std::vector<Consumer::SubscriptionConfig> subscriptions) : Session(config, moq::SessionMode::SubscribeOnly), subscriptions_(std::move(subscriptions)) {}

 private:
  std::vector<Consumer::SubscriptionConfig> subscriptions_;
  std::vector<std::unique_ptr<Consumer>> consumers_;
  
  // Announcement management
  std::unique_ptr<moq::OriginConsumer> origin_consumer_;
  std::thread announcement_thread_;
  std::map<std::string, std::unique_ptr<Consumer>> announced_consumers_;  // path -> consumer
  
  void announcement_loop();

 protected: 
  void start_all_workers() override;
  void stop_all_workers() override;
  void cleanup_connections() override;
};

}  // namespace moq_mgr