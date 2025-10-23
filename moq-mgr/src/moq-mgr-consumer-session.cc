#include "../include/moq-mgr/session.h"
#include <nlohmann/json.hpp>
#include <unordered_set>

using json = nlohmann::json;

namespace moq_mgr {

void ConsumerSession::handle_connected() {
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

  while (running_ && origin_consumer) {
    try {
      auto announce = origin_consumer->announced();

      if (announce) {
        const std::string& path = announce->path;
        
        if (announce->active) {
          notify_status("Started consumer for announced broadcast: " + path);
          if (path == config_.moq_namespace) {
            moq_consumer_ = std::move(moq_session_->consume(config_.moq_namespace));
            if (!moq_consumer_) {
              notify_error("Failed to create BroadcastConsumer for namespace: " + config_.moq_namespace);
              continue;
            }
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
  catalog_subscription.data_callback = [this](uint8_t* data, size_t size) {
    process_catalog_data(data, size);
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

void ConsumerSession::process_catalog_data(const uint8_t* data, size_t size) {
  try {
    // Convert raw data to string
    std::string json_str(reinterpret_cast<const char*>(data), size);
    
    // Parse JSON
    json catalog_json = json::parse(json_str);
    
    // Check if tracks array exists
    if (!catalog_json.contains("tracks") || !catalog_json["tracks"].is_array()) {
      notify_error("Catalog data missing 'tracks' array");
      return;
    }
    
    // Process each track
    const auto& tracks = catalog_json["tracks"];
    notify_status("Received catalog with " + std::to_string(tracks.size()) + " tracks:");

    available_tracks_.clear();
    for (const auto& track : tracks) {
      if (track.contains("trackName") && track.contains("type") && track.contains("priority")) {
        std::string track_name = track["trackName"].get<std::string>();
        std::string track_type = track["type"].get<std::string>();
        int track_priority = track["priority"].get<int>();

        // Store available track information
        available_tracks_[track_name] = AvailableTrack{track_name, track_type, track_priority};
        
        notify_status("  - Track: " + track_name + 
                     " (type: " + track_type + 
                     ", priority: " + std::to_string(track_priority) + ")");

                     check_subscriptions();
      } else {
        notify_status("  - Skipping track with missing trackName, type, or priority");
      }
    }
    
  } catch (const json::parse_error& e) {
    notify_error("Failed to parse catalog JSON: " + std::string(e.what()));
  } catch (const json::exception& e) {
    notify_error("Error processing catalog: " + std::string(e.what()));
  } catch (const std::exception& e) {
    notify_error("Unexpected error processing catalog: " + std::string(e.what()));
  }
}

void ConsumerSession::check_subscriptions() {
  std::lock_guard<std::mutex> lock(mutex_);

  // First pass: Remove consumers for tracks that are no longer available
  auto it = consumers_.begin();
  while (it != consumers_.end()) {
    if (!*it) {
      it = consumers_.erase(it);
      continue;
    }

    const std::string& track_name = (*it)->get_track_name();
    
    // If this track is not in available_tracks_, unsubscribe
    if (available_tracks_.find(track_name) == available_tracks_.end()) {
      notify_status("Unsubscribing from track no longer available: " + track_name);
      (*it)->stop();
      it = consumers_.erase(it);
    } else {
      ++it;
    }
  }

  // Second pass: Add new consumers for requested tracks that are now available
  for (const auto& [track_name, requested_config] : requested_subscriptions_) {
    // Check if this track is available
    if (available_tracks_.find(track_name) == available_tracks_.end()) {
      continue;  // Not available yet
    }

    // Check if we're already subscribed to this track
    bool already_subscribed = false;
    for (const auto& consumer : consumers_) {
      if (consumer && consumer->get_track_name() == track_name) {
        already_subscribed = true;
        break;
      }
    }

    // If not already subscribed, start a new subscription
    if (!already_subscribed) {
      notify_status("Starting subscription to newly available track: " + track_name);
      
      auto consumer = std::make_unique<Consumer>(moq_consumer_, requested_config);
      consumer->start();
      consumers_.push_back(std::move(consumer));
    }
  }
}

}  // namespace moq_mgr