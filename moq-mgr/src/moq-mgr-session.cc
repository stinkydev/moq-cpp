#include "moq-mgr/session.h"

#include <atomic>
#include <chrono>
#include <condition_variable>
#include <map>
#include <mutex>
#include <thread>

#include <moq/moq.h>

namespace moq_mgr {

Session::Session(const SessionConfig& config, moq::SessionMode mode)
    : config_(config), mode_(mode) {
  notify_status("Session initialized");
}

Session::~Session() { 
}

bool Session::start() {
  {
    std::lock_guard<std::mutex> lock(mutex_);
    if (running_) {
      return true;
    }

    running_ = true;

    // Create MoQ client and connect
    moq::ClientConfig cfg;
    cfg.bind_addr = "0.0.0.0:0";
    moq_client_ = moq::Client::create(cfg);

    if (!moq_client_) {
      notify_error("Failed to create MoQ client");
      running_ = false;
      return false;
    }

    auto session = moq_client_->connect(config_.moq_server, moq::SessionMode::Both);
    if (!session || !session->isConnected()) {
      notify_error("Failed to connect to MoQ server: " + config_.moq_server);
      running_ = false;
      return false;
    }
    moq_session_ = std::move(session);

    try {
      start_all_workers();

      // Start session management thread
      session_thread_ = std::thread(&Session::session_loop, this);

      notify_status("Session started");
      return true;
    } catch (const std::exception& e) {
      running_ = false;
      notify_error("Failed to start session: " + std::string(e.what()));
      return false;
    }
  }
}

void Session::stop() {
  {
    std::lock_guard<std::mutex> lock(mutex_);
    if (!running_) {
      return;
    }

    running_ = false;
  }

  if (moq_client_) moq_session_->close();
  
  // Notify all threads to wake up
  condition_.notify_all();

  stop_all_workers();

  if (session_thread_.joinable()) {
    session_thread_.join();
  }

  cleanup_connections();

  notify_status("MoQ Session stopped");
}

bool Session::is_running() const {
  std::lock_guard<std::mutex> lock(mutex_);
  return running_;
}

void Session::set_error_callback(std::function<void(const std::string&)> callback) {
  error_callback_ = std::move(callback);
}

void Session::set_status_callback(std::function<void(const std::string&)> callback) {
  status_callback_ = std::move(callback);
}

void Session::session_loop() {
  
  while (running_) {
    try {
      // Monitor session health and manage workers
      if (moq_session_ && !moq_session_->isAlive()) {
        notify_error("MoQ session disconnected, attempting reconnection...");
        
        if (!config_.reconnect_on_failure) {
          notify_error("Reconnection disabled, stopping session");
          stop();
          break;
        }
    
        // Check if enough time has passed since the last reconnection attempt
        auto now = std::chrono::steady_clock::now();
        bool should_attempt_reconnect = first_reconnect_attempt_;
        
        if (!first_reconnect_attempt_) {
          auto time_since_last_attempt = std::chrono::duration_cast<std::chrono::seconds>(
              now - last_reconnect_attempt_).count();
          should_attempt_reconnect = (time_since_last_attempt >= 3);
        }
        
        if (should_attempt_reconnect) {
          last_reconnect_attempt_ = now;
          first_reconnect_attempt_ = false;
          
          // Attempt to reconnect
          if (reconnect()) {
            notify_status("Successfully reconnected to MoQ server");
            first_reconnect_attempt_ = true; // Reset for next disconnection
          } else {
            notify_error("Failed to reconnect to MoQ server, will retry in 3 seconds...");
          }
        }
      }

      std::unique_lock<std::mutex> lock(mutex_);
      condition_.wait_for(lock, std::chrono::seconds(1),
                          [this] { return !running_; });
    } catch (const std::exception& e) {
      notify_error("Session loop error: " + std::string(e.what()));
    }
  }
}

bool Session::reconnect() {
  std::lock_guard<std::mutex> lock(mutex_);
  
  if (!running_) {
    return false;
  }

  try {
    // Stop all workers first
    stop_all_workers();

    // Close and cleanup old connection
    if (moq_session_) {
      moq_session_->close();
      moq_session_.reset();
    }

    // Create new MoQ client if needed
    if (!moq_client_) {
      moq::ClientConfig cfg;
      cfg.bind_addr = "0.0.0.0:0";
      moq_client_ = moq::Client::create(cfg);
      
      if (!moq_client_) {
        notify_error("Failed to recreate MoQ client during reconnection");
        return false;
      }
    }

    // Attempt to reconnect
    auto session = moq_client_->connect(config_.moq_server, mode_);
    if (!session || !session->isConnected()) {
      notify_error("Failed to reconnect to MoQ server: " + config_.moq_server);
      return false;
    }

    // Set the new session
    moq_session_ = std::move(session);

    // Restart all workers with the new session
    start_all_workers();

    return true;

  } catch (const std::exception& e) {
    notify_error("Exception during reconnection: " + std::string(e.what()));
    return false;
  }
}

void ConsumerSession::start_all_workers() {
  consumers_.clear();

  for (size_t i = 0; i < subscriptions_.size(); ++i) {
    const auto& subscription_config = subscriptions_[i];
    
    // Create a consumer for this specific subscription
    auto consumer = std::make_unique<Consumer>(
        i, config_.moq_namespace, subscription_config, moq_session_);
    
    consumer->start();
    consumers_.push_back(std::move(consumer));
  }
}

void ProducerSession::start_all_workers() {
  producers_.clear();

  for (size_t i = 0; i < broadcasts_.size(); ++i) {
    const auto& broadcast_config = broadcasts_[i];

    // Create a producer for this specific broadcast
    auto producer = std::make_unique<Producer>(
        i, config_.moq_namespace, broadcast_config, moq_session_);
    
    producer->start();
    producers_.push_back(std::move(producer));
  }
}

void ConsumerSession::cleanup_connections() {
  consumers_.clear();
  moq_session_.reset();
  moq_client_.reset();
}

void ConsumerSession::stop_all_workers() {
  // Stop all consumers (they will join their own threads)
  for (auto& consumer : consumers_) {
    if (consumer) {
      consumer->stop();
    }
  }
}

void ProducerSession::cleanup_connections() {
  producers_.clear();
  moq_session_.reset();
  moq_client_.reset();
}

void ProducerSession::stop_all_workers() {
  // Stop all producers (they will join their own threads)
  for (auto& producer : producers_) {
    if (producer) {
      producer->stop();
    }
  }
}

void Session::notify_error(const std::string& error) {
  if (error_callback_) {
    error_callback_(error);
  }
}

void Session::notify_status(const std::string& status) {
  if (status_callback_) {
    status_callback_(status);
  }
}

}  // namespace moq_cro_bridge