#include "../include/moq-mgr/session.h"

namespace moq_mgr {

void ConsumerSession::handle_connected() {
  moq_consumer_ = moq_session_->consume(config_.moq_namespace);
  announcement_thread_ = std::thread(&ConsumerSession::announcement_loop, this);
}

void ConsumerSession::handle_disconnected() {
  if (announcement_thread_.joinable()) {
    announcement_thread_.join();
  }
}

void ConsumerSession::announcement_loop() {
  auto origin_consumer = moq_session_->getOriginConsumer();
  if (!origin_consumer) {
    notify_error("Failed to get OriginConsumer for announcements");
    return;
  }

  size_t next_consumer_id = subscriptions_.size();

  while (running_ && origin_consumer) {
    try {
      auto announce = origin_consumer->announced();

      if (announce) {
        const std::string& path = announce->path;
        
        if (announce->active) {
          notify_status("Started consumer for announced broadcast: " + path);
          if (path == config_.moq_namespace) {
            start_catalog_consumer();
          }
        } else {
          notify_status("Stopped consumer for announced broadcast: " + path);
          if (path == config_.moq_namespace) {
            stop_catalog_consumer();
          }
        }
      } else {
        // No announcements available, sleep briefly to avoid busy waiting
        std::this_thread::sleep_for(std::chrono::milliseconds(1));
      }
    } catch (const std::exception& e) {
      if (running_) {
        notify_error("Error in announcement loop: " + std::string(e.what()));
        std::this_thread::sleep_for(std::chrono::seconds(1));
      }
    }
  }
  
  notify_status("Announcement monitoring stopped");
}

void ConsumerSession::start_catalog_consumer() {
  Consumer::SubscriptionConfig catalog_subscription = {};
  catalog_subscription.moq_track_name = "catalog";
  catalog_subscription.data_callback = [&](uint8_t* data, size_t size) {
    // Handle catalog data (for now, just print size)
   notify_status("Received catalog data of size: " + std::to_string(size) + " bytes");
  };

  this->catalog_consumer = std::make_unique<Consumer>(moq_consumer_, catalog_subscription);
  this->catalog_consumer->start();
  notify_status("Catalog consumer started");
}

void ConsumerSession::stop_catalog_consumer() {
  catalog_consumer->stop();
  catalog_consumer.reset();
  notify_status("Catalog consumer stopped");
}

}  // namespace moq_mgr