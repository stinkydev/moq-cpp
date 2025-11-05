#include "moq_wrapper.h"

#include <cstring>
#include <iostream>
#include <map>

// Forward declarations for C FFI functions
extern "C" {
void moq_init(int log_level, void (*log_callback)(const char*, int, const char*));
void* moq_track_definition_new(const char* name, uint32_t priority, int track_type);
void moq_track_definition_free(void* track_def);
void* moq_create_publisher(const char* url, const char* broadcast_name,
                          void** tracks, size_t track_count, int catalog_type);
void* moq_create_subscriber(const char* url, const char* broadcast_name,
                           void** tracks, size_t track_count, int catalog_type);
int moq_set_data_callback(void* session,
                         void (*callback)(const char*, const uint8_t*, size_t));
int moq_write_single_frame(void* session, const char* track_name, 
                          const uint8_t* data, size_t data_len);
int moq_write_frame(void* session, const char* track_name,
                   const uint8_t* data, size_t data_len, int new_group);
int moq_is_connected(void* session);
int moq_close_session(void* session);
void moq_session_free(void* session);
}

namespace moq {

namespace {
// Global log callback storage
LogCallback g_log_callback;

// Global data callback storage for each session
std::map<void*, DataCallback> g_data_callbacks;

// C wrapper for log callback
extern "C" void LogCallbackWrapper(const char* target, int level,
                                   const char* message) {
  if (g_log_callback) {
    g_log_callback(std::string(target), static_cast<LogLevel>(level),
                   std::string(message));
  }
}

// C wrapper for data callback
extern "C" void DataCallbackWrapper(const char* track, const uint8_t* data,
                                    size_t size) {
  // This will be set up per session in SetDataCallback
}

}  // namespace

TrackDefinition::TrackDefinition(const std::string& name, uint32_t priority,
                                 TrackType track_type)
    : name_(name), priority_(priority), track_type_(track_type) {
  handle_ = moq_track_definition_new(name.c_str(), priority,
                                     static_cast<int>(track_type));
}

TrackDefinition::~TrackDefinition() {
  if (handle_) {
    moq_track_definition_free(handle_);
  }
}

void Init(LogLevel log_level, const LogCallback& log_callback) {
  g_log_callback = log_callback;
  
  if (log_callback) {
    moq_init(static_cast<int>(log_level), LogCallbackWrapper);
  } else {
    moq_init(static_cast<int>(log_level), nullptr);
  }
}

std::unique_ptr<Session> Session::CreatePublisher(
    const std::string& url, const std::string& broadcast_name,
    const std::vector<TrackDefinition>& tracks, CatalogType catalog_type) {
  // Prepare track handles
  std::vector<void*> track_handles;
  track_handles.reserve(tracks.size());
  for (const auto& track : tracks) {
    track_handles.push_back(track.GetHandle());
  }

  void* handle = moq_create_publisher(
      url.c_str(), broadcast_name.c_str(),
      track_handles.empty() ? nullptr : track_handles.data(),
      track_handles.size(), static_cast<int>(catalog_type));

  if (!handle) {
    return nullptr;
  }

  return std::unique_ptr<Session>(new Session(handle));
}

std::unique_ptr<Session> Session::CreateSubscriber(
    const std::string& url, const std::string& broadcast_name,
    const std::vector<TrackDefinition>& tracks, CatalogType catalog_type) {
  // Prepare track handles
  std::vector<void*> track_handles;
  track_handles.reserve(tracks.size());
  for (const auto& track : tracks) {
    track_handles.push_back(track.GetHandle());
  }

  void* handle = moq_create_subscriber(
      url.c_str(), broadcast_name.c_str(),
      track_handles.empty() ? nullptr : track_handles.data(),
      track_handles.size(), static_cast<int>(catalog_type));

  if (!handle) {
    return nullptr;
  }

  return std::unique_ptr<Session>(new Session(handle));
}

Session::Session(void* handle) : handle_(handle) {}

Session::~Session() {
  if (handle_) {
    moq_session_free(handle_);
  }
}

bool Session::SetDataCallback(const DataCallback& callback) {
  if (!handle_) {
    return false;
  }

  // Store the callback globally for this session
  g_data_callbacks[handle_] = callback;

  // Create a C wrapper that will call our stored callback
  auto c_callback = [](const char* track, const uint8_t* data, size_t size) {
    // Find the session handle - this is a limitation of the current design
    // In a real implementation, you'd pass the session handle through the callback
    for (const auto& pair : g_data_callbacks) {
      // For now, we'll call all callbacks - this should be improved
      pair.second(std::string(track), data, size);
    }
  };

  return moq_set_data_callback(handle_, c_callback) == 0;
}

bool Session::WriteFrame(const std::string& track_name, const uint8_t* data, 
                        size_t size, bool new_group) {
  if (!handle_) {
    return false;
  }
  
  return moq_write_frame(handle_, track_name.c_str(), data, size, new_group ? 1 : 0) == 0;
}

bool Session::WriteSingleFrame(const std::string& track_name, const uint8_t* data, size_t size) {
  if (!handle_) {
    return false;
  }
  return moq_write_single_frame(handle_, track_name.c_str(), data, size) == 0;
}

bool Session::IsConnected() const {
  if (!handle_) {
    return false;
  }
  return moq_is_connected(handle_) != 0;
}

bool Session::Close() {
  if (!handle_) {
    return false;
  }
  return moq_close_session(handle_) == 0;
}

}  // namespace moq