#include "moq/client.h"
#include "moq/session.h"

// Include the generated C header
extern "C" {
    #include "moq_ffi.h"
}

#include <stdexcept>

namespace moq {

// Static initialization
Result Client::initialize() {
    auto result = moq_init();
    return static_cast<Result>(result);
}

// Factory method to create a client
std::unique_ptr<Client> Client::create(const ClientConfig& config) {
    MoqClientConfig c_config = {};
    c_config.bind_addr = config.bind_addr.empty() ? nullptr : config.bind_addr.c_str();
    c_config.tls_disable_verify = config.tls_disable_verify;
    c_config.tls_root_cert_path = config.tls_root_cert_path.empty() ? nullptr : config.tls_root_cert_path.c_str();

    MoqClient* client_handle = nullptr;
    auto result = moq_client_new(&c_config, &client_handle);
    
    if (result != MoqResult::Success || client_handle == nullptr) {
        return nullptr;
    }

    return std::unique_ptr<Client>(new Client(client_handle));
}

// Private constructor
Client::Client(void* handle) : handle_(handle) {}

// Destructor
Client::~Client() {
    if (handle_) {
        moq_client_free(static_cast<MoqClient*>(handle_));
    }
}

// Connect method
std::unique_ptr<Session> Client::connect(const std::string& url) {
    if (!handle_) {
        return nullptr;
    }

    MoqSession* session_handle = nullptr;
    auto result = moq_client_connect(
        static_cast<MoqClient*>(handle_),
        url.c_str(),
        &session_handle
    );

    if (result != MoqResult::Success || session_handle == nullptr) {
        return nullptr;
    }

    return std::unique_ptr<Session>(new Session(session_handle));
}

// Get last error
std::string Client::getLastError() const {
    const char* error = moq_get_last_error();
    return error ? std::string(error) : std::string();
}

// Convert result to string
std::string Client::resultToString(Result result) {
    const char* str = moq_result_to_string(static_cast<MoqResult>(result));
    return str ? std::string(str) : std::string("Unknown result");
}

} // namespace moq
