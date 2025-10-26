#include <moq/moq.h>
#include <iostream>
#include <chrono>
#include <thread>
#include <atomic>
#include <memory>

class CatalogTest {
private:
    std::string url_;
    std::string broadcast_name_;
    std::string track_name_;
    std::atomic<bool> running_;
    std::unique_ptr<moq::Client> client_;
    std::shared_ptr<moq::Session> session_;

public:
    CatalogTest(const std::string& url, const std::string& broadcast_name, const std::string& track_name)
        : url_(url), broadcast_name_(broadcast_name), track_name_(track_name), running_(true) {}

    bool initialize() {
        // Initialize the MOQ library
        auto init_result = moq::Client::initialize();
        if (init_result != moq::Result::Success) {
            std::cerr << "Failed to initialize MOQ library: " 
                      << moq::Client::resultToString(init_result) << std::endl;
            return false;
        }

        std::cout << "MOQ library initialized successfully" << std::endl;
        return true;
    }

    bool run() {
        std::cout << "Connecting to relay: " << url_ << std::endl;
        std::cout << "Waiting for broadcast: " << broadcast_name_ << std::endl;
        std::cout << "Will subscribe to track: " << track_name_ << std::endl;

        // Create MOQ client with configuration
        moq::ClientConfig config;
        config.bind_addr = "0.0.0.0:0";  // Use IPv4
        
        client_ = moq::Client::create(config);
        if (!client_) {
            std::cerr << "Failed to create MOQ client" << std::endl;
            return false;
        }

        // Connect to the relay with subscribe-only mode
        session_ = client_->connect(url_, moq::SessionMode::SubscribeOnly);
        if (!session_) {
            std::cerr << "Failed to connect to relay: " << client_->getLastError() << std::endl;
            return false;
        }

        std::cout << "Successfully connected to MOQ server!" << std::endl;

        // Get origin consumer for announcements
        auto origin_consumer = session_->getOriginConsumer();
        if (!origin_consumer) {
            std::cerr << "Failed to get origin consumer" << std::endl;
            return false;
        }

        std::cout << "Waiting for '" << broadcast_name_ << "' broadcast to be announced..." << std::endl;

        // Wait for the broadcast to be announced as active
        std::unique_ptr<moq::BroadcastConsumer> broadcast_consumer;
        while (running_ && session_->isAlive()) {
            auto announcement = origin_consumer->announced();
            
            if (announcement) {
                std::cout << "Received announcement: path='" << announcement->path 
                          << "', active=" << (announcement->active ? "true" : "false") << std::endl;

                if (announcement->path == broadcast_name_ && announcement->active) {
                    std::cout << "Broadcast '" << broadcast_name_ << "' is now active!" << std::endl;
                    
                    // Consume the broadcast
                    std::cout << "Consuming broadcast '" << broadcast_name_ << "'" << std::endl;
                    broadcast_consumer = session_->consume(broadcast_name_);
                    
                    if (!broadcast_consumer) {
                        std::cerr << "Failed to consume broadcast" << std::endl;
                        return false;
                    }
                    
                    break;
                }
            } else {
                // No announcement available, sleep briefly
                std::this_thread::sleep_for(std::chrono::milliseconds(10));
            }
        }

        if (!broadcast_consumer) {
            std::cerr << "Failed to get broadcast consumer" << std::endl;
            return false;
        }

        // Subscribe to the catalog track
        std::cout << "Subscribing to track '" << track_name_ << "'" << std::endl;
        
        moq::Track track;
        track.name = track_name_;
        track.priority = 0;
        
        auto track_consumer = broadcast_consumer->subscribeTrack(track);
        if (!track_consumer) {
            std::cerr << "Failed to subscribe to track" << std::endl;
            return false;
        }

        std::cout << "Subscribed to track, waiting for data..." << std::endl;

        // Wait for the first group
        auto group_future = track_consumer->nextGroup();
        
        // Wait with timeout
        auto status = group_future.wait_for(std::chrono::seconds(10));
        if (status == std::future_status::timeout) {
            std::cerr << "Timeout waiting for group" << std::endl;
            return false;
        }

        auto group_consumer = group_future.get();
        if (!group_consumer) {
            std::cerr << "No group available" << std::endl;
            return false;
        }

        std::cout << "Received group, reading first frame..." << std::endl;

        // Read the first frame
        auto frame_future = group_consumer->readFrame();
        
        // Wait with timeout
        status = frame_future.wait_for(std::chrono::seconds(5));
        if (status == std::future_status::timeout) {
            std::cerr << "Timeout waiting for frame" << std::endl;
            return false;
        }

        auto frame_data = frame_future.get();
        if (!frame_data || frame_data->empty()) {
            std::cerr << "No frame data available" << std::endl;
            return false;
        }

        std::cout << "Successfully read frame! Size: " << frame_data->size() << " bytes" << std::endl;

        // Try to display as text if it looks like UTF-8
        std::string text(frame_data->begin(), frame_data->end());
        std::cout << "Frame payload (as text):" << std::endl;
        std::cout << text << std::endl;

        std::cout << "Test completed successfully, exiting" << std::endl;
        return true;
    }

    void stop() {
        running_ = false;
        if (session_) {
            session_->close();
        }
    }
};

void printUsage(const char* program_name) {
    std::cout << "Usage: " << program_name << " [OPTIONS]\n\n"
              << "Options:\n"
              << "  --url <url>          MOQ relay URL (default: https://relay1.moq.sesame-streams.com:4433)\n"
              << "  --broadcast <name>   Broadcast name to wait for (default: peter)\n"
              << "  --track <name>       Track name to subscribe to (default: catalog)\n"
              << "  --help               Show this help message\n\n"
              << "Example:\n"
              << "  " << program_name << " --url https://relay1.moq.sesame-streams.com:4433 --broadcast peter --track catalog\n"
              << std::endl;
}

int main(int argc, char* argv[]) {
    // Default values
    std::string url = "https://relay1.moq.sesame-streams.com:4433";
    std::string broadcast_name = "peter";
    std::string track_name = "catalog.json";

    // Parse command line arguments
    for (int i = 1; i < argc; i++) {
        std::string arg = argv[i];
        
        if (arg == "--url" && i + 1 < argc) {
            url = argv[++i];
        } else if (arg == "--broadcast" && i + 1 < argc) {
            broadcast_name = argv[++i];
        } else if (arg == "--track" && i + 1 < argc) {
            track_name = argv[++i];
        } else if (arg == "--help") {
            printUsage(argv[0]);
            return 0;
        } else {
            std::cerr << "Unknown argument: " << arg << std::endl;
            printUsage(argv[0]);
            return 1;
        }
    }

    std::cout << "MOQ Catalog Test Application" << std::endl;
    std::cout << "============================" << std::endl;
    std::cout << "URL: " << url << std::endl;
    std::cout << "Broadcast: " << broadcast_name << std::endl;
    std::cout << "Track: " << track_name << std::endl;
    std::cout << std::endl;

    // Create and run the test
    CatalogTest test(url, broadcast_name, track_name);
    
    if (!test.initialize()) {
        std::cerr << "Failed to initialize application" << std::endl;
        return 1;
    }

    if (!test.run()) {
        std::cerr << "Application failed" << std::endl;
        return 1;
    }

    return 0;
}
