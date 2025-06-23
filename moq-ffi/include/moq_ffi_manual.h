#ifndef MOQ_FFI_MANUAL_H
#define MOQ_FFI_MANUAL_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

// Opaque types - forward declarations only
typedef struct MoqClient MoqClient;
typedef struct MoqSession MoqSession;
typedef struct MoqTrack MoqTrack;

// Data callback type
typedef void (*MoqDataCallback)(const char *track_name,
                                const uint8_t *data,
                                size_t data_len,
                                void *user_data);

// Result enum
typedef enum {
    Success = 0,
    InvalidArgument = 1,
    NetworkError = 2,
    TlsError = 3,
    DnsError = 4,
    GeneralError = 5,
} MoqResult;

// Configuration struct
typedef struct {
    const char *bind_addr;
    bool tls_disable_verify;
    const char *tls_root_cert_path;
} MoqClientConfig;

// Function declarations
MoqResult moq_init(void);
MoqResult moq_client_new(const MoqClientConfig *config, MoqClient **client_out);
void moq_client_free(MoqClient *client);
MoqResult moq_client_connect(MoqClient *client, const char *url, MoqSession **session_out);
const char *moq_client_get_last_error(MoqClient *client);

bool moq_session_is_connected(const MoqSession *session);
MoqResult moq_session_close(MoqSession *session);
void moq_session_free(MoqSession *session);
MoqResult moq_session_publish_track(MoqSession *session,
                                    const char *track_name,
                                    MoqTrack **track_out);
MoqResult moq_session_subscribe_track(MoqSession *session,
                                      const char *track_name,
                                      MoqDataCallback callback,
                                      void *user_data,
                                      MoqTrack **track_out);

MoqResult moq_track_send_data(MoqTrack *track, const uint8_t *data, size_t data_len);
void moq_track_free(MoqTrack *track);

const char *moq_get_last_error(void);
const char *moq_result_to_string(MoqResult result);

#ifdef __cplusplus
}
#endif

#endif // MOQ_FFI_MANUAL_H
