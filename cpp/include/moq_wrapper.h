#ifndef MOQ_WRAPPER_H
#define MOQ_WRAPPER_H

#include <cstdint>
#include <functional>
#include <memory>
#include <mutex>
#include <string>
#include <vector>

// Windows DLL export/import macros
#ifdef _WIN32
#ifdef BUILDING_MOQ_CPP
#define MOQ_API __declspec(dllexport)
#else
#define MOQ_API __declspec(dllimport)
#endif
// Suppress warnings about STL containers in DLL interface
#pragma warning(push)
#pragma warning(disable : 4251)
#else
#define MOQ_API
#endif

namespace moq
{

  /// Log levels
  enum class LogLevel
  {
    kTrace = 0,
    kDebug = 1,
    kInfo = 2,
    kWarn = 3,
    kError = 4
  };

  /// Track types
  enum class TrackType
  {
    kVideo = 0,
    kAudio = 1,
    kData = 2
  };

  /// Catalog types
  enum class CatalogType
  {
    kNone = 0,
    kSesame = 1,
    kHang = 2
  };

  /// Log callback function type
  using LogCallback = std::function<void(const std::string &target, LogLevel level,
                                         const std::string &message)>;

  /// Data callback function type
  using DataCallback =
      std::function<void(const std::string &track, const uint8_t *data,
                         size_t size)>;

  /// Track definition
  class MOQ_API TrackDefinition
  {
  public:
    TrackDefinition(const std::string &name, uint32_t priority,
                    TrackType track_type);
    ~TrackDefinition();

    const std::string &name() const { return name_; }
    uint32_t priority() const { return priority_; }
    TrackType track_type() const { return track_type_; }

    // Internal handle for FFI
    void *GetHandle() const { return handle_; }

  private:
    std::string name_;
    uint32_t priority_;
    TrackType track_type_;
    void *handle_;
  };

  /// Forward declaration for friend function
  extern "C" void SessionDataCallbackWrapper(void *, const char *, const uint8_t *, size_t);

  /// MOQ Session wrapper
  class MOQ_API Session
  {
    friend void SessionDataCallbackWrapper(void *, const char *, const uint8_t *, size_t);

  public:
    /// Create a publisher session
    static std::unique_ptr<Session> CreatePublisher(
        const std::string &url, const std::string &broadcast_name,
        const std::vector<TrackDefinition> &tracks,
        CatalogType catalog_type = CatalogType::kNone);

    /// Create a subscriber session
    static std::unique_ptr<Session> CreateSubscriber(
        const std::string &url, const std::string &broadcast_name,
        const std::vector<TrackDefinition> &tracks,
        CatalogType catalog_type = CatalogType::kNone);

    ~Session();

    /// Set data callback for receiving track data
    bool SetDataCallback(const DataCallback &callback);

    /// Set log callback for receiving session-specific log messages
    bool SetLogCallback(const LogCallback &callback);

    /// Write a frame to a track, optionally starting a new group
    /// @param track_name Name of the track
    /// @param data Pointer to the data
    /// @param size Size of the data
    /// @param new_group If true, starts a new group before writing the frame
    bool WriteFrame(const std::string &track_name, const uint8_t *data,
                    size_t size, bool new_group = false);

    /// Write a single frame in its own group (convenience method)
    /// Creates a new group, writes the frame, and closes the group
    bool WriteSingleFrame(const std::string &track_name, const uint8_t *data, size_t size);

    /// Check if session is connected
    bool IsConnected() const;

    /// Close the session
    bool Close();

  private:
    explicit Session(void *handle);

    void *handle_;
    std::mutex callback_mutex_;
    std::unique_ptr<DataCallback> data_callback_;
  };

  /// Set the global log level for internal library tracing (optional)
  /// This configures global tracing for internal diagnostics.
  /// Session-specific logging is handled via Session::SetLogCallback().
  /// @param log_level The log level for internal library tracing
  MOQ_API void SetLogLevel(LogLevel log_level);

} // namespace moq

#ifdef _WIN32
#pragma warning(pop)
#endif

#endif // MOQ_WRAPPER_H