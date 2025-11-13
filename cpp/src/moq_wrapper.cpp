#include "moq_wrapper.h"

#include <cstring>
#include <iostream>
#include <map>
#include <mutex>
#include <thread>
#include <unordered_map>

// C-compatible track definition structure
struct TrackDefinitionFFI
{
  const char *name;
  uint32_t priority;
  uint8_t track_type;
};

// Forward declarations for C FFI functions
extern "C"
{
  void moq_set_log_level(int log_level, void (*log_callback)(const char *, int, const char *));
  void *moq_track_definition_new(const char *name, uint32_t priority, int track_type);
  void moq_track_definition_free(void *track_def);
  void *moq_create_publisher(const char *url, const char *broadcast_name,
                             const TrackDefinitionFFI *tracks, size_t track_count, int catalog_type);
  void *moq_create_subscriber(const char *url, const char *broadcast_name,
                              const TrackDefinitionFFI *tracks, size_t track_count, int catalog_type);
  int moq_session_set_data_callback(void *session,
                                    void (*callback)(void *, const char *, const uint8_t *, size_t));
  int moq_write_single_frame(void *session, const char *track_name,
                             const uint8_t *data, size_t data_len);
  int moq_write_frame(void *session, const char *track_name,
                      const uint8_t *data, size_t data_len, int new_group);
  int moq_is_connected(void *session);
  int moq_close_session(void *session);
  void moq_session_free(void *session);
  int moq_session_set_log_callback(void *session, void (*callback)(const char *, int, const char *));
  int moq_session_set_broadcast_announced_callback(void *session, void (*callback)(const char *));
  int moq_session_set_broadcast_cancelled_callback(void *session, void (*callback)(const char *));
  int moq_session_set_connection_closed_callback(void *session, void (*callback)(const char *));
}

namespace moq
{

  namespace
  {
    // Thread-safe global log callback storage
    std::mutex g_callback_mutex;
    LogCallback g_log_callback;

    // Session-specific data callback storage
    std::unordered_map<void *, Session *> g_session_map;
    std::mutex g_session_map_mutex;

    // Global callback instances (set per session)
    Session *g_current_session = nullptr;
    BroadcastAnnouncedCallback *g_broadcast_announced_callback = nullptr;
    BroadcastCancelledCallback *g_broadcast_cancelled_callback = nullptr;
    ConnectionClosedCallback *g_connection_closed_callback = nullptr;

    // Thread-safe C wrapper for log callback
    extern "C" void LogCallbackWrapper(const char *target, int level,
                                       const char *message)
    {
      std::lock_guard<std::mutex> lock(g_callback_mutex);
      if (g_log_callback)
      {
        g_log_callback(std::string(target), static_cast<LogLevel>(level),
                       std::string(message));
      }
    }

    // C wrapper for data callback
    extern "C" void DataCallbackWrapper(const char *track, const uint8_t *data,
                                        size_t size)
    {
      // This will be set up per session in SetDataCallback
      // Suppress unused parameter warnings
      (void)track;
      (void)data;
      (void)size;
    }

  } // namespace

  // C wrapper functions for new callbacks - outside anonymous namespace to access globals
  extern "C" void SessionBroadcastAnnouncedWrapper(const char *path)
  {
    std::lock_guard<std::mutex> lock(g_session_map_mutex);
    if (g_current_session && g_current_session->broadcast_announced_callback_)
    {
      (*g_current_session->broadcast_announced_callback_)(std::string(path));
    }
  }

  extern "C" void SessionBroadcastCancelledWrapper(const char *path)
  {
    std::lock_guard<std::mutex> lock(g_session_map_mutex);
    if (g_current_session && g_current_session->broadcast_cancelled_callback_)
    {
      (*g_current_session->broadcast_cancelled_callback_)(std::string(path));
    }
  }

  extern "C" void SessionConnectionClosedWrapper(const char *reason)
  {
    std::lock_guard<std::mutex> lock(g_session_map_mutex);
    if (g_current_session && g_current_session->connection_closed_callback_)
    {
      (*g_current_session->connection_closed_callback_)(std::string(reason));
    }

  } // namespace

  TrackDefinition::TrackDefinition(const std::string &name, uint32_t priority,
                                   TrackType track_type)
      : name_(name), priority_(priority), track_type_(track_type)
  {
    handle_ = moq_track_definition_new(name.c_str(), priority,
                                       static_cast<int>(track_type));
  }

  TrackDefinition::~TrackDefinition()
  {
    if (handle_)
    {
      moq_track_definition_free(handle_);
    }
  }

  // Copy constructor - creates a new Rust handle
  TrackDefinition::TrackDefinition(const TrackDefinition &other)
      : name_(other.name_), priority_(other.priority_), track_type_(other.track_type_)
  {
    // Create a new Rust handle for the copy
    handle_ = moq_track_definition_new(name_.c_str(), priority_,
                                       static_cast<int>(track_type_));
  }

  // Copy assignment operator
  TrackDefinition &TrackDefinition::operator=(const TrackDefinition &other)
  {
    if (this != &other)
    {
      // Free existing handle
      if (handle_)
      {
        moq_track_definition_free(handle_);
      }

      // Copy data
      name_ = other.name_;
      priority_ = other.priority_;
      track_type_ = other.track_type_;

      // Create new Rust handle
      handle_ = moq_track_definition_new(name_.c_str(), priority_,
                                         static_cast<int>(track_type_));
    }
    return *this;
  }

  // Move constructor - transfers ownership of Rust handle
  TrackDefinition::TrackDefinition(TrackDefinition &&other) noexcept
      : name_(std::move(other.name_)), priority_(other.priority_),
        track_type_(other.track_type_), handle_(other.handle_)
  {
    // Take ownership of handle
    other.handle_ = nullptr;
  }

  // Move assignment operator
  TrackDefinition &TrackDefinition::operator=(TrackDefinition &&other) noexcept
  {
    if (this != &other)
    {
      // Free existing handle
      if (handle_)
      {
        moq_track_definition_free(handle_);
      }

      // Move data
      name_ = std::move(other.name_);
      priority_ = other.priority_;
      track_type_ = other.track_type_;
      handle_ = other.handle_;

      // Take ownership
      other.handle_ = nullptr;
    }
    return *this;
  }

  void SetLogLevel(LogLevel log_level)
  {
    // Set global tracing level for internal library diagnostics
    moq_set_log_level(static_cast<int>(log_level), nullptr);
  }

  std::unique_ptr<Session> Session::CreatePublisher(
      const std::string &url, const std::string &broadcast_name,
      const std::vector<TrackDefinition> &tracks, CatalogType catalog_type)
  {
    // Prepare FFI track definitions
    // Keep strings alive by storing them separately
    std::vector<std::string> track_names;
    std::vector<TrackDefinitionFFI> ffi_tracks;
    track_names.reserve(tracks.size());
    ffi_tracks.reserve(tracks.size());

    for (const auto &track : tracks)
    {
      track_names.push_back(track.name());
      ffi_tracks.push_back({track_names.back().c_str(),
                            track.priority(),
                            static_cast<uint8_t>(track.track_type())});
    }

    void *handle = moq_create_publisher(
        url.c_str(), broadcast_name.c_str(),
        ffi_tracks.empty() ? nullptr : ffi_tracks.data(),
        ffi_tracks.size(), static_cast<int>(catalog_type));

    if (!handle)
    {
      return nullptr;
    }

    return std::unique_ptr<Session>(new Session(handle));
  }

  std::unique_ptr<Session> Session::CreateSubscriber(
      const std::string &url, const std::string &broadcast_name,
      const std::vector<TrackDefinition> &tracks, CatalogType catalog_type)
  {
    // Prepare FFI track definitions
    // Keep strings alive by storing them separately
    std::vector<std::string> track_names;
    std::vector<TrackDefinitionFFI> ffi_tracks;
    track_names.reserve(tracks.size());
    ffi_tracks.reserve(tracks.size());

    for (const auto &track : tracks)
    {
      track_names.push_back(track.name());
      ffi_tracks.push_back({track_names.back().c_str(),
                            track.priority(),
                            static_cast<uint8_t>(track.track_type())});
    }

    void *handle = moq_create_subscriber(
        url.c_str(), broadcast_name.c_str(),
        ffi_tracks.empty() ? nullptr : ffi_tracks.data(),
        ffi_tracks.size(), static_cast<int>(catalog_type));

    if (!handle)
    {
      return nullptr;
    }

    return std::unique_ptr<Session>(new Session(handle));
  }

  Session::Session(void *handle) : handle_(handle)
  {
    // Register this session instance with the handle
    std::lock_guard<std::mutex> lock(g_session_map_mutex);
    g_session_map[handle_] = this;

    // Set this as the current session for callback routing
    g_current_session = this;
  }

  Session::~Session()
  {
    if (handle_)
    {
      // Clear the callbacks first
      {
        std::lock_guard<std::mutex> lock(callback_mutex_);
        data_callback_.reset();
        broadcast_announced_callback_.reset();
        broadcast_cancelled_callback_.reset();
        connection_closed_callback_.reset();
      }

      // Unregister from session map
      {
        std::lock_guard<std::mutex> lock(g_session_map_mutex);
        g_session_map.erase(handle_);
      }

      // Close the session first to ensure proper cleanup
      moq_close_session(handle_);

      // Small delay to allow cleanup to complete
      std::this_thread::sleep_for(std::chrono::milliseconds(10));

      // Free the session
      moq_session_free(handle_);
    }
  }

  // Session-specific data callback wrapper
  extern "C" void SessionDataCallbackWrapper(void *ffi_session_ptr, const char *track, const uint8_t *data, size_t size)
  {
    if (!ffi_session_ptr)
      return;

    Session *session = nullptr;
    {
      std::lock_guard<std::mutex> lock(g_session_map_mutex);
      auto it = g_session_map.find(ffi_session_ptr);
      if (it != g_session_map.end())
      {
        session = it->second;
      }
    }

    if (session && session->data_callback_)
    {
      try
      {
        (*session->data_callback_)(std::string(track), data, size);
      }
      catch (const std::exception &e)
      {
        std::cerr << "Exception in data callback: " << e.what() << std::endl;
      }
      catch (...)
      {
        std::cerr << "Unknown exception in data callback" << std::endl;
      }
    }
  }

  bool Session::SetDataCallback(const DataCallback &callback)
  {
    if (!handle_)
    {
      return false;
    }

    // Store the callback in this session instance
    {
      std::lock_guard<std::mutex> lock(callback_mutex_);
      data_callback_ = std::make_unique<DataCallback>(callback);
    }

    // Set the session-specific callback function, passing 'this' as context
    return moq_session_set_data_callback(handle_, SessionDataCallbackWrapper) == 0;
  }

  bool Session::SetLogCallback(const LogCallback &callback)
  {
    if (!handle_)
    {
      return false;
    }

    // Thread-safe storage of the callback
    {
      std::lock_guard<std::mutex> lock(g_callback_mutex);
      g_log_callback = callback;
    }

    if (callback)
    {
      return moq_session_set_log_callback(handle_, LogCallbackWrapper) == 0;
    }
    else
    {
      return moq_session_set_log_callback(handle_, nullptr) == 0;
    }
  }

  bool Session::SetBroadcastAnnouncedCallback(const BroadcastAnnouncedCallback &callback)
  {
    if (!handle_)
    {
      return false;
    }

    // Store the callback in this session instance
    {
      std::lock_guard<std::mutex> lock(callback_mutex_);
      broadcast_announced_callback_ = std::make_unique<BroadcastAnnouncedCallback>(callback);
    }

    // Set the callback in the Rust session
    return moq_session_set_broadcast_announced_callback(handle_, SessionBroadcastAnnouncedWrapper) == 0;
  }

  bool Session::SetBroadcastCancelledCallback(const BroadcastCancelledCallback &callback)
  {
    if (!handle_)
    {
      return false;
    }

    // Store the callback in this session instance
    {
      std::lock_guard<std::mutex> lock(callback_mutex_);
      broadcast_cancelled_callback_ = std::make_unique<BroadcastCancelledCallback>(callback);
    }

    // Set the callback in the Rust session
    return moq_session_set_broadcast_cancelled_callback(handle_, SessionBroadcastCancelledWrapper) == 0;
  }

  bool Session::SetConnectionClosedCallback(const ConnectionClosedCallback &callback)
  {
    if (!handle_)
    {
      return false;
    }

    // Store the callback in this session instance
    {
      std::lock_guard<std::mutex> lock(callback_mutex_);
      connection_closed_callback_ = std::make_unique<ConnectionClosedCallback>(callback);
    }

    // Set the callback in the Rust session
    return moq_session_set_connection_closed_callback(handle_, SessionConnectionClosedWrapper) == 0;
  }

  bool Session::WriteFrame(const std::string &track_name, const uint8_t *data,
                           size_t size, bool new_group)
  {
    if (!handle_)
    {
      return false;
    }

    return moq_write_frame(handle_, track_name.c_str(), data, size, new_group ? 1 : 0) == 0;
  }

  bool Session::WriteSingleFrame(const std::string &track_name, const uint8_t *data, size_t size)
  {
    if (!handle_)
    {
      return false;
    }
    return moq_write_single_frame(handle_, track_name.c_str(), data, size) == 0;
  }

  bool Session::IsConnected() const
  {
    if (!handle_)
    {
      return false;
    }
    return moq_is_connected(handle_) != 0;
  }

  bool Session::Close()
  {
    if (!handle_)
    {
      return false;
    }
    return moq_close_session(handle_) == 0;
  }

} // namespace moq