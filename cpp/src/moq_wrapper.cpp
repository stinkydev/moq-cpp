#include "moq_wrapper.h"

#include <cstring>
#include <iostream>
#include <map>
#include <mutex>
#include <thread>
#include <unordered_map>
#include <utility>

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
                              const TrackDefinitionFFI *tracks, size_t track_count, int catalog_type,
                              int subscribe_all_catalog_tracks);
  void *moq_create_room_subscriber(const char *url, const char *room_prefix,
                                   const TrackDefinitionFFI *tracks, size_t track_count, int catalog_type,
                                   int subscribe_all_catalog_tracks);
  int moq_session_set_data_callback(void *session,
                                    void (*callback)(void *, const char *, const uint8_t *, size_t));
  int moq_write_single_frame(void *session, const char *track_name,
                             const uint8_t *data, size_t data_len);
  int moq_write_frame(void *session, const char *track_name,
                      const uint8_t *data, size_t data_len, int new_group);
  int moq_publish_data(void *session, const char *track_name,
                       const uint8_t *data, size_t data_len);
  int moq_is_connected(void *session);
  int moq_close_session(void *session);
  void moq_session_free(void *session);
  int moq_session_set_log_callback(void *session, void (*callback)(const char *, int, const char *));
  int moq_session_set_broadcast_announced_callback(void *session, void (*callback)(void *, const char *));
  int moq_session_set_broadcast_cancelled_callback(void *session, void (*callback)(void *, const char *));
  int moq_session_set_connection_closed_callback(void *session, void (*callback)(void *, const char *));
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

  } // namespace

  namespace
  {
    // A Session looked up from the FFI handle, together with the held session
    // map lock. While the lock is held the Session cannot be destroyed.
    struct LockedSession
    {
      std::unique_lock<std::mutex> lock;
      Session *session = nullptr;
    };

    LockedSession FindSession(void *ffi_session_ptr)
    {
      LockedSession result;
      if (!ffi_session_ptr)
      {
        return result;
      }

      result.lock = std::unique_lock<std::mutex>(g_session_map_mutex);
      auto it = g_session_map.find(ffi_session_ptr);
      if (it != g_session_map.end())
      {
        result.session = it->second;
      }
      return result;
    }

    // Copy a callback out of its storage under the session's callback mutex.
    // The copy owns its captures, so it stays valid after the Session is gone.
    template <typename Callback>
    Callback CopyCallback(std::mutex &callback_mutex, const std::unique_ptr<Callback> &callback)
    {
      std::lock_guard<std::mutex> lock(callback_mutex);
      return callback ? *callback : Callback();
    }

    template <typename Callback, typename... Args>
    void InvokeCallback(const char *name, const Callback &callback, Args &&...args)
    {
      if (!callback)
      {
        return;
      }

      try
      {
        callback(std::forward<Args>(args)...);
      }
      catch (const std::exception &e)
      {
        std::cerr << "Exception in " << name << " callback: " << e.what() << std::endl;
      }
      catch (...)
      {
        std::cerr << "Unknown exception in " << name << " callback" << std::endl;
      }
    }
  } // namespace

  // C wrapper functions for session callbacks - outside anonymous namespace to
  // access Session private members via friendship. Each one resolves the Session
  // from the FFI pointer supplied by Rust, copies the callback while the session
  // is pinned by the map lock, and invokes the copy with no locks held so that
  // callbacks may freely create or destroy sessions.
  extern "C" void SessionBroadcastAnnouncedWrapper(void *ffi_session_ptr, const char *path)
  {
    BroadcastAnnouncedCallback callback;
    {
      auto locked = FindSession(ffi_session_ptr);
      if (locked.session)
      {
        callback = CopyCallback(locked.session->callback_mutex_,
                                locked.session->broadcast_announced_callback_);
      }
    }
    InvokeCallback("broadcast announced", callback, std::string(path));
  }

  extern "C" void SessionBroadcastCancelledWrapper(void *ffi_session_ptr, const char *path)
  {
    BroadcastCancelledCallback callback;
    {
      auto locked = FindSession(ffi_session_ptr);
      if (locked.session)
      {
        callback = CopyCallback(locked.session->callback_mutex_,
                                locked.session->broadcast_cancelled_callback_);
      }
    }
    InvokeCallback("broadcast cancelled", callback, std::string(path));
  }

  extern "C" void SessionConnectionClosedWrapper(void *ffi_session_ptr, const char *reason)
  {
    ConnectionClosedCallback callback;
    {
      auto locked = FindSession(ffi_session_ptr);
      if (locked.session)
      {
        callback = CopyCallback(locked.session->callback_mutex_,
                                locked.session->connection_closed_callback_);
      }
    }
    InvokeCallback("connection closed", callback, std::string(reason));
  }

  extern "C" void SessionDataCallbackWrapper(void *ffi_session_ptr, const char *track, const uint8_t *data, size_t size)
  {
    DataCallback callback;
    {
      auto locked = FindSession(ffi_session_ptr);
      if (locked.session)
      {
        callback = CopyCallback(locked.session->callback_mutex_,
                                locked.session->data_callback_);
      }
    }
    InvokeCallback("data", callback, std::string(track), data, size);
  }

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
      const std::vector<TrackDefinition> &tracks, CatalogType catalog_type,
      bool subscribe_all_catalog_tracks)
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
        ffi_tracks.size(), static_cast<int>(catalog_type),
        subscribe_all_catalog_tracks ? 1 : 0);

    if (!handle)
    {
      return nullptr;
    }

    return std::unique_ptr<Session>(new Session(handle));
  }

  std::unique_ptr<Session> Session::CreateRoomSubscriber(
      const std::string &url, const std::string &room_prefix,
      const std::vector<TrackDefinition> &tracks, CatalogType catalog_type,
      bool subscribe_all_catalog_tracks)
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

    void *handle = moq_create_room_subscriber(
        url.c_str(), room_prefix.c_str(),
        ffi_tracks.empty() ? nullptr : ffi_tracks.data(),
        ffi_tracks.size(), static_cast<int>(catalog_type),
        subscribe_all_catalog_tracks ? 1 : 0);

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

      // Unregister from session map so no further callbacks resolve to this session
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

  bool Session::PublishData(const std::string &track_name, const uint8_t *data, size_t size)
  {
    if (!handle_)
    {
      return false;
    }
    return moq_publish_data(handle_, track_name.c_str(), data, size) == 0;
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
