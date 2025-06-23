#pragma once

#include <memory>
#include <string>
#include <functional>

namespace moq {

/// Forward declarations
class Session;
class Track;

/// Result enumeration matching the C FFI
enum class Result {
    Success = 0,
    InvalidArgument = 1,
    NetworkError = 2,
    TlsError = 3,
    DnsError = 4,
    GeneralError = 5
};

/// Configuration for MOQ client
struct ClientConfig {
    std::string bind_addr = "[::]:0";
    bool tls_disable_verify = false;
    std::string tls_root_cert_path = "";
};

/// MOQ Client class - provides a C++ interface to the MOQ native client
class Client {
public:
    /// Initialize the MOQ library (call once before creating any clients)
    static Result initialize();

    /// Create a new MOQ client with the given configuration
    /// @param config Configuration for the client
    static std::unique_ptr<Client> create(const ClientConfig& config);

    /// Destructor
    ~Client();

    /// Connect to a MOQ server
    /// @param url URL to connect to
    /// @return Unique pointer to a Session on success, nullptr on failure
    std::unique_ptr<Session> connect(const std::string& url);

    /// Get the last error that occurred
    /// @return Error message string, empty if no error
    std::string getLastError() const;

    /// Convert a Result code to a human-readable string
    /// @param result The result code to convert
    /// @return String description of the result
    static std::string resultToString(Result result);

private:
    Client(void* handle);
    void* handle_;
};

} // namespace moq
