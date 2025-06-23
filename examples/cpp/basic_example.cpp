#include <iostream>
#include <memory>
#include <moq/moq.h>

int main() {
    // Initialize the MOQ library
    auto init_result = moq::Client::initialize();
    if (init_result != moq::Result::Success) {
        std::cerr << "Failed to initialize MOQ library: " 
                  << moq::Client::resultToString(init_result) << std::endl;
        return 1;
    }

    std::cout << "MOQ library initialized successfully" << std::endl;

    // Create client configuration
    moq::ClientConfig config;
    config.bind_addr = "[::]:0";  // Bind to any available port
    config.tls_disable_verify = false;  // Enable TLS verification for security

    // Create the client
    auto client = moq::Client::create(config);
    if (!client) {
        std::cerr << "Failed to create MOQ client" << std::endl;
        return 1;
    }

    std::cout << "MOQ client created successfully" << std::endl;

    // Example URL - replace with your actual MOQ server URL
    std::string server_url = "https://relay.quic.video:443";

    std::cout << "Attempting to connect to: " << server_url << std::endl;

    // Connect to the server
    auto session = client->connect(server_url);
    if (!session) {
        std::cerr << "Failed to connect to MOQ server" << std::endl;
        std::string error = client->getLastError();
        if (!error.empty()) {
            std::cerr << "Error: " << error << std::endl;
        }
        return 1;
    }

    std::cout << "Successfully connected to MOQ server!" << std::endl;

    // Check connection status
    if (session->isConnected()) {
        std::cout << "Session is active and connected" << std::endl;
    }

    // In a real application, you would:
    // 1. Subscribe to tracks
    // 2. Publish tracks
    // 3. Handle incoming data
    // 4. Manage the session lifecycle

    std::cout << "Example completed successfully" << std::endl;

    // Session and client will be automatically cleaned up when they go out of scope
    return 0;
}
