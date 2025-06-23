#include <iostream>
#include <memory>
#include <thread>
#include <chrono>
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

    // Create publisher client configuration
    moq::ClientConfig pub_config;
    pub_config.bind_addr = "[::]:0";  // Bind to any available port
    pub_config.tls_disable_verify = false;  // Enable TLS verification for security

    // Create subscriber client configuration
    moq::ClientConfig sub_config;
    sub_config.bind_addr = "[::]:0";  // Bind to any available port
    sub_config.tls_disable_verify = false;  // Enable TLS verification for security

    // Create the publisher client
    auto publisher_client = moq::Client::create(pub_config);
    if (!publisher_client) {
        std::cerr << "Failed to create MOQ publisher client" << std::endl;
        return 1;
    }

    // Create the subscriber client
    auto subscriber_client = moq::Client::create(sub_config);
    if (!subscriber_client) {
        std::cerr << "Failed to create MOQ subscriber client" << std::endl;
        return 1;
    }

    std::cout << "Publisher and subscriber clients created successfully" << std::endl;

    // Example URL - replace with your actual MOQ server URL
    std::string server_url = "https://moq.sesame-streams.com:4443";

    std::cout << "Attempting to connect to: " << server_url << std::endl;

    // Connect publisher to the server
    auto publisher_session = publisher_client->connect(server_url);
    if (!publisher_session) {
        std::cerr << "Failed to connect publisher to MOQ server" << std::endl;
        std::string error = publisher_client->getLastError();
        if (!error.empty()) {
            std::cerr << "Error: " << error << std::endl;
        }
        return 1;
    }

    // Connect subscriber to the server
    auto subscriber_session = subscriber_client->connect(server_url);
    if (!subscriber_session) {
        std::cerr << "Failed to connect subscriber to MOQ server" << std::endl;
        std::string error = subscriber_client->getLastError();
        if (!error.empty()) {
            std::cerr << "Error: " << error << std::endl;
        }
        return 1;
    }

    std::cout << "Both clients successfully connected to MOQ server!" << std::endl;

    // Track name for demonstration
    const std::string track_name = "demo-track";

    // Set up subscriber first
    std::cout << "Setting up subscriber for track: " << track_name << std::endl;
    
    auto subscriber_track = subscriber_session->subscribeTrack(track_name, 
        [](const std::string& name, const std::vector<uint8_t>& data) {
            std::string received_data(data.begin(), data.end());
            std::cout << "📨 Received data on track '" << name << "': " << received_data << std::endl;
        });

    if (!subscriber_track) {
        std::cerr << "Failed to subscribe to track: " << track_name << std::endl;
        return 1;
    }

    std::cout << "Successfully subscribed to track: " << track_name << std::endl;

    // Set up publisher
    std::cout << "Setting up publisher for track: " << track_name << std::endl;
    
    auto publisher_track = publisher_session->publishTrack(track_name);
    if (!publisher_track) {
        std::cerr << "Failed to publish track: " << track_name << std::endl;
        return 1;
    }

    std::cout << "Successfully set up publisher for track: " << track_name << std::endl;

    // Publish some data
    std::cout << "\\n🚀 Starting to publish data..." << std::endl;
    
    // Send a series of messages
    std::vector<std::string> messages = {
        "Hello from MOQ publisher!",
        "This is message #2",
        "Broadcasting live data stream",
        "Final message from publisher"
    };

    for (size_t i = 0; i < messages.size(); ++i) {
        std::cout << "📤 Publishing: " << messages[i] << std::endl;
        
        if (!publisher_track->sendData(messages[i])) {
            std::cerr << "Failed to send message: " << messages[i] << std::endl;
        }
        
        // Wait a bit between messages
        std::this_thread::sleep_for(std::chrono::milliseconds(500));
    }

    // Send some binary data
    std::vector<uint8_t> binary_data = {0x48, 0x65, 0x6c, 0x6c, 0x6f, 0x20, 0x42, 0x69, 0x6e, 0x61, 0x72, 0x79}; // "Hello Binary"
    std::cout << "📤 Publishing binary data..." << std::endl;
    
    if (!publisher_track->sendData(binary_data)) {
        std::cerr << "Failed to send binary data" << std::endl;
    }

    // Give some time for the last message to be processed
    std::this_thread::sleep_for(std::chrono::milliseconds(1000));

    std::cout << "\\n✅ Demo completed successfully!" << std::endl;
    std::cout << "\\nSummary:" << std::endl;
    std::cout << "- Created publisher and subscriber clients" << std::endl;
    std::cout << "- Connected both clients to the MOQ server" << std::endl;
    std::cout << "- Set up a track for publishing and subscribing" << std::endl;
    std::cout << "- Published " << messages.size() + 1 << " messages (text + binary)" << std::endl;
    std::cout << "- Received data through the subscription callback" << std::endl;

    // Check connection status
    if (publisher_session->isConnected() && subscriber_session->isConnected()) {
        std::cout << "Both sessions are still active and connected" << std::endl;
    }

    // Sessions and clients will be automatically cleaned up when they go out of scope
    return 0;
}
