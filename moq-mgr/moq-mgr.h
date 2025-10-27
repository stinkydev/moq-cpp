#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

/**
 * Result codes for FFI functions
 */
typedef enum MoqMgrResult {
  Success = 0,
  ErrorInvalidParameter = -1,
  ErrorNotConnected = -2,
  ErrorAlreadyConnected = -3,
  ErrorInternal = -4,
} MoqMgrResult;

/**
 * Opaque handle to a MoQ Manager session
 */
typedef struct MoqMgrSession MoqMgrSession;

/**
 * Error callback function type
 * Parameters: error_message (null-terminated C string), user_data
 */
typedef void (*MoqMgrErrorCallback)(const char*, void*);

/**
 * Status callback function type
 * Parameters: status_message (null-terminated C string), user_data
 */
typedef void (*MoqMgrStatusCallback)(const char*, void*);

/**
 * Data callback function type for consumers
 * Parameters: data pointer, data length, user_data
 */
typedef void (*MoqMgrDataCallback)(const uint8_t*, uintptr_t, void*);

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

/**
 * Initialize the MoQ Manager library
 * This should be called once at startup
 */
enum MoqMgrResult moq_mgr_init(void);

/**
 * Create a new MoQ Manager session
 *
 * Parameters:
 * - server_url: The MoQ server URL (e.g., "https://relay.moq.example.com:4433")
 * - namespace: The broadcast namespace to use
 * - mode: Session mode (0=PublishOnly, 1=SubscribeOnly, 2=PublishAndSubscribe)
 * - reconnect: Whether to automatically reconnect on failure (0=false, 1=true)
 *
 * Returns: Pointer to MoqMgrSession or null on error
 */
struct MoqMgrSession *moq_mgr_session_create(const char *server_url,
                                             const char *namespace_,
                                             int32_t mode,
                                             int32_t reconnect);

/**
 * Create a new MoQ Manager session with custom bind address
 *
 * Parameters:
 * - server_url: The MoQ server URL (e.g., "https://relay.moq.example.com:4433")
 * - namespace: The broadcast namespace to use
 * - mode: Session mode (0=PublishOnly, 1=SubscribeOnly, 2=PublishAndSubscribe)
 * - reconnect: Whether to automatically reconnect on failure (0=false, 1=true)
 * - bind_addr: Optional bind address (e.g., "0.0.0.0:0" for IPv4, null for default)
 *
 * Returns: Pointer to MoqMgrSession or null on error
 */
struct MoqMgrSession *moq_mgr_session_create_with_bind(const char *server_url,
                                                       const char *namespace_,
                                                       int32_t mode,
                                                       int32_t reconnect,
                                                       const char *bind_addr);

/**
 * Set error callback for the session
 */
enum MoqMgrResult moq_mgr_session_set_error_callback(struct MoqMgrSession *session,
                                                     MoqMgrErrorCallback callback,
                                                     void *user_data);

/**
 * Set status callback for the session
 */
enum MoqMgrResult moq_mgr_session_set_status_callback(struct MoqMgrSession *session,
                                                      MoqMgrStatusCallback callback,
                                                      void *user_data);

/**
 * Add a subscription to the session (for consumer mode)
 * Must be called before moq_mgr_session_start
 */
enum MoqMgrResult moq_mgr_session_add_subscription(struct MoqMgrSession *session,
                                                   const char *track_name,
                                                   MoqMgrDataCallback callback,
                                                   void *user_data);

/**
 * Add a broadcast to the session (for producer mode)
 * Must be called before moq_mgr_session_start
 */
enum MoqMgrResult moq_mgr_session_add_broadcast(struct MoqMgrSession *session,
                                                const char *track_name,
                                                uint32_t priority);

/**
 * Start the session and connect to the MoQ server
 */
enum MoqMgrResult moq_mgr_session_start(struct MoqMgrSession *session);

/**
 * Stop the session and disconnect from the MoQ server
 */
enum MoqMgrResult moq_mgr_session_stop(struct MoqMgrSession *session);

/**
 * Check if the session is running
 */
int32_t moq_mgr_session_is_running(struct MoqMgrSession *session);

/**
 * Destroy the session and free all resources
 */
void moq_mgr_session_destroy(struct MoqMgrSession *session);

/**
 * Get the last error message (thread-local)
 */
const char *moq_mgr_get_last_error(void);

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus
