#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

/**
 * Result codes for MOQ operations
 */
typedef enum MoqResult {
    Success = 0,
    InvalidArgument = 1,
    NetworkError = 2,
    TlsError = 3,
    DnsError = 4,
    GeneralError = 5,
} MoqResult;

/**
 * Internal client configuration
 */
typedef struct ClientConfigData ClientConfigData;

typedef struct String String;

/**
 * Configuration for MOQ client
 */
typedef struct MoqClientConfig {
    const char *bind_addr;
    bool tls_disable_verify;
    const char *tls_root_cert_path;
} MoqClientConfig;

/**
 * Opaque handle for the MOQ client
 */
typedef struct MoqClient {
    uint64_t runtime_handle;
    struct ClientConfigData config;
} MoqClient;

/**
 * Opaque handle for a MOQ session
 */
typedef struct MoqSession {
    uint64_t connection_id;
    bool is_connected;
    struct String server_url;
} MoqSession;

/**
 * Opaque handle for a MOQ track
 */
typedef struct MoqTrack {
    uint64_t session_id;
    struct String name;
    bool is_publisher;
} MoqTrack;

/**
 * Data callback for received track data
 */
typedef void (*MoqDataCallback)(const char *track_name,
                                const uint8_t *data,
                                uintptr_t data_len,
                                void *user_data);

/**
 * Initialize the MOQ FFI library
 * This should be called once before using any other functions
 */
enum MoqResult moq_init(void);

/**
 * Create a new MOQ client with the given configuration
 *
 * # Arguments
 * * `config` - Configuration for the client
 * * `client_out` - Output parameter for the created client handle
 *
 * # Returns
 * * `MoqResult` indicating success or failure
 *
 * # Safety
 * This function is unsafe because it dereferences raw pointers.
 * The caller must ensure that:
 * - `config` points to valid MoqClientConfig
 * - `client_out` points to valid memory for a MoqClient pointer
 */
enum MoqResult moq_client_new(const struct MoqClientConfig *config, struct MoqClient **client_out);

/**
 * Connect to a MOQ server
 *
 * # Arguments
 * * `client` - Handle to the MOQ client
 * * `url` - URL to connect to (as C string)
 * * `session_out` - Output parameter for the created session handle
 *
 * # Returns
 * * `MoqResult` indicating success or failure
 *
 * # Safety
 * This function is unsafe because it dereferences raw pointers.
 * The caller must ensure that:
 * - `client` points to a valid MoqClient
 * - `url` points to a valid null-terminated C string
 * - `session_out` points to valid memory for a MoqSession pointer
 */
enum MoqResult moq_client_connect(struct MoqClient *client,
                                  const char *url,
                                  struct MoqSession **session_out);

/**
 * Free a MOQ client handle
 *
 * # Arguments
 * * `client` - Handle to the MOQ client to free
 *
 * # Safety
 * This function is unsafe because it dereferences a raw pointer.
 * The caller must ensure that `client` is a valid pointer obtained from `moq_client_new`.
 */
void moq_client_free(struct MoqClient *client);

/**
 * Free a MOQ session handle
 *
 * # Arguments
 * * `session` - Handle to the MOQ session to free
 *
 * # Safety
 * This function is unsafe because it dereferences a raw pointer.
 * The caller must ensure that `session` is a valid pointer obtained from `moq_client_connect`.
 */
void moq_session_free(struct MoqSession *session);

/**
 * Check if a session is connected
 */
bool moq_session_is_connected(const struct MoqSession *session);

/**
 * Close a MOQ session
 */
enum MoqResult moq_session_close(struct MoqSession *session);

/**
 * Publish a track
 */
enum MoqResult moq_session_publish_track(struct MoqSession *session,
                                         const char *track_name,
                                         struct MoqTrack **track_out);

/**
 * Subscribe to a track
 */
enum MoqResult moq_session_subscribe_track(struct MoqSession *session,
                                           const char *track_name,
                                           MoqDataCallback callback,
                                           void *user_data,
                                           struct MoqTrack **track_out);

/**
 * Send data on a published track
 */
enum MoqResult moq_track_send_data(struct MoqTrack *track, const uint8_t *data, uintptr_t data_len);

/**
 * Free a MOQ track handle
 *
 * # Arguments
 * * `track` - Handle to the MOQ track to free
 */
void moq_track_free(struct MoqTrack *track);

/**
 * Get the last error message (thread-local)
 *
 * # Returns
 * * Pointer to a null-terminated string containing the error message
 * * The returned string is valid until the next call to any MOQ function
 * * Returns null if no error occurred
 */
const char *moq_get_last_error(void);

/**
 * Convert a MoqResult to a human-readable string
 *
 * # Arguments
 * * `result` - The result code to convert
 *
 * # Returns
 * * Pointer to a null-terminated string describing the result
 */
const char *moq_result_to_string(enum MoqResult result);
