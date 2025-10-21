#include <moq/moq.h>
#include <iostream>
#include <chrono>
#include <thread>
#include <atomic>
#include <memory>
#include <future>
#include <iomanip>
#include <sstream>
#include <map>
#include <conio.h>  // For _kbhit() and _getch() on Windows

class TrackSubscriber {
private:
    std::unique_ptr<moq::TrackConsumer> track_;
    std::string track_name_;
    std::atomic<uint64_t> bytes_received_;
    std::atomic<bool> running_;
    std::atomic<bool> cancelled_;

public:
    TrackSubscriber(std::unique_ptr<moq::TrackConsumer> track, const std::string& track_name) 
        : track_(std::move(track)), track_name_(track_name), bytes_received_(0), running_(true), cancelled_(false) {}

    void run() {
        std::cout << "Starting subscriber thread for track: " << track_name_ << std::endl;
        uint64_t group_count = 0;
        uint64_t consecutive_timeouts = 0;
        const uint64_t MAX_TIMEOUTS_BEFORE_ASSUME_NO_DATA = 10; // 5 seconds of timeouts
        
        while (running_ && !cancelled_ && track_) {
            try {
                // Check if track consumer is still valid
                if (!track_) {
                    std::cout << "Track " << track_name_ << ": Track consumer was destroyed" << std::endl;
                    break;
                }
                
                // Get the next group with timeout to allow for cancellation
                auto group_future = track_->nextGroup();
                
                // Wait with shorter timeout to allow more responsive cancellation
                auto status = group_future.wait_for(std::chrono::milliseconds(200));
                if (status == std::future_status::timeout) {
                    consecutive_timeouts++;
                    
                    // If we haven't received any data after many timeouts, assume no data
                    if (group_count == 0 && consecutive_timeouts >= MAX_TIMEOUTS_BEFORE_ASSUME_NO_DATA) {
                        std::cout << "Track " << track_name_ << ": No data received after " 
                                  << (consecutive_timeouts * 200) << "ms, assuming no data available" << std::endl;
                        break;
                    }
                    
                    // Check for cancellation more frequently
                    if (cancelled_ || !track_) {
                        std::cout << "Track " << track_name_ << ": Cancelled during timeout" << std::endl;
                        break;
                    }
                    continue;
                } else if (status == std::future_status::ready) {
                    consecutive_timeouts = 0; // Reset timeout counter
                    auto group = group_future.get();
                    
                    if (!group) {
                        std::cout << "Track " << track_name_ << ": No more groups available (received " << group_count << " groups total)" << std::endl;
                        break;
                    }

                    group_count++;
                    uint64_t group_bytes = 0;

                    // Read all frames in the group
                    int frame_count = 0;
                    while (running_ && !cancelled_ && track_) {
                        if (!track_) {
                            std::cout << "Track " << track_name_ << ": Track consumer destroyed during frame reading" << std::endl;
                            break;
                        }
                        
                        auto frame_future = group->readFrame();
                        
                        // Also add timeout for frame reading
                        auto frame_status = frame_future.wait_for(std::chrono::milliseconds(100));
                        if (frame_status == std::future_status::timeout) {
                            // Check for cancellation
                            if (cancelled_ || !track_) {
                                std::cout << "Track " << track_name_ << ": Cancelled during frame read" << std::endl;
                                break;
                            }
                            continue;
                        } else if (frame_status == std::future_status::ready) {
                            auto frame_data = frame_future.get();
                            
                            if (!frame_data || frame_data->empty()) {
                                break;
                            }

                            // Count the bytes received
                            uint64_t frame_size = frame_data->size();
                            bytes_received_ += frame_size;
                            group_bytes += frame_size;
                            frame_count++;
                        } else {
                            // Future failed
                            break;
                        }
                    }
                    
                    // Log summary for each group
                    if (running_ && !cancelled_ && track_) {
                        std::cout << "Track " << track_name_ << ": Group " << group_count 
                                  << " - " << frame_count << " frames, " << group_bytes 
                                  << " bytes (total: " << bytes_received_ << " bytes)" << std::endl;
                    }
                } else {
                    // Future failed
                    break;
                }
            } catch (const std::exception& e) {
                std::cerr << "Error in track " << track_name_ << " subscriber: " << e.what() << std::endl;
                if (cancelled_ || !track_) {
                    break;
                }
                std::this_thread::sleep_for(std::chrono::seconds(1));
                continue;
            }
        }
        
        std::cout << "Track " << track_name_ << " subscriber finished. Groups: " << group_count 
                  << ", Total bytes: " << bytes_received_ << std::endl;
    }

    void stop() {
        running_ = false;
        cancelled_ = true;
        
        // Immediately destroy the track consumer to trigger Rust cleanup
        if (track_) {
            std::cout << "Destroying track consumer for: " << track_name_ << std::endl;
            track_.reset();  // This will call the destructor and free the Rust consumer
        }
    }

    bool isCancelled() const {
        return cancelled_;
    }

    uint64_t getBytesReceived() const {
        return bytes_received_;
    }

    const std::string& getTrackName() const {
        return track_name_;
    }
};

class RelayTestApp {
private:
    std::string url_;
    std::string broadcast_name_;
    std::vector<std::string> available_track_names_;
    std::map<std::string, std::unique_ptr<TrackSubscriber>> active_subscribers_;
    std::map<std::string, std::thread> subscriber_threads_;
    std::atomic<bool> running_;
    
    // MOQ objects
    std::unique_ptr<moq::Client> client_;
    std::unique_ptr<moq::Session> session_;
    std::unique_ptr<moq::BroadcastConsumer> broadcast_consumer_;
    bool is_connected_;

public:
    RelayTestApp(const std::string& url, const std::string& broadcast_name, 
                 const std::vector<std::string>& track_names)
        : url_(url), broadcast_name_(broadcast_name), available_track_names_(track_names), 
          running_(true), is_connected_(false) {}

    ~RelayTestApp() {
        stop();
    }

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

    bool connectToRelay() {
        if (is_connected_) {
            std::cout << "Already connected to relay" << std::endl;
            return true;
        }

        // Create client configuration
        moq::ClientConfig config;
        config.bind_addr = "0.0.0.0:0";
        config.tls_disable_verify = true;

        // Create the client
        client_ = moq::Client::create(config);
        if (!client_) {
            std::cerr << "Failed to create MOQ client" << std::endl;
            return false;
        }

        std::cout << "Connecting to: " << url_ << std::endl;

        // Connect to the server in subscribe-only mode
        session_ = client_->connect(url_, moq::SessionMode::SubscribeOnly);
        if (!session_) {
            std::cerr << "Failed to connect to MOQ server" << std::endl;
            std::string error = client_->getLastError();
            if (!error.empty()) {
                std::cerr << "Error: " << error << std::endl;
            }
            return false;
        }

        std::cout << "Successfully connected to MOQ server!" << std::endl;

        // Give some time for the broadcast to be available
        std::cout << "Waiting for broadcast to be available..." << std::endl;
        std::this_thread::sleep_for(std::chrono::seconds(2));

        // Consume the broadcast
        std::cout << "Consuming broadcast: " << broadcast_name_ << std::endl;
        broadcast_consumer_ = session_->consume(broadcast_name_);
        if (!broadcast_consumer_) {
            std::cerr << "Failed to consume broadcast '" << broadcast_name_ 
                      << "' (maybe no publisher available?)" << std::endl;
            return false;
        }

        std::cout << "Successfully consuming broadcast!" << std::endl;
        is_connected_ = true;
        return true;
    }

    void disconnectFromRelay() {
        if (!is_connected_) {
            std::cout << "Not connected to relay" << std::endl;
            return;
        }

        std::cout << "Disconnecting from relay..." << std::endl;
        
        // Stop all active subscribers
        unsubscribeFromAllTracks();
        
        // Close session
        if (session_) {
            session_->close();
            session_.reset();
        }
        
        // Reset client
        client_.reset();
        broadcast_consumer_.reset();
        
        is_connected_ = false;
        std::cout << "Disconnected from relay" << std::endl;
    }

    bool subscribeToTrack(const std::string& track_name) {
        if (!is_connected_ || !broadcast_consumer_) {
            std::cout << "Not connected to relay. Cannot subscribe to track: " << track_name << std::endl;
            return false;
        }

        if (active_subscribers_.find(track_name) != active_subscribers_.end()) {
            std::cout << "Already subscribed to track: " << track_name << std::endl;
            return true;
        }

        std::cout << "Subscribing to track: " << track_name << std::endl;
        
        moq::Track track;
        track.name = track_name;
        track.priority = 0;

        auto track_consumer = broadcast_consumer_->subscribeTrack(track);
        if (!track_consumer) {
            std::cerr << "Failed to subscribe to track: " << track_name << std::endl;
            return false;
        }

        std::cout << "Successfully subscribed to track: " << track_name << std::endl;

        // Create subscriber for this track
        auto subscriber = std::make_unique<TrackSubscriber>(std::move(track_consumer), track_name);
        
        // Store subscriber first
        active_subscribers_[track_name] = std::move(subscriber);

        // Start subscriber thread
        subscriber_threads_[track_name] = std::thread([this, track_name]() {
            active_subscribers_[track_name]->run();
        });
        return true;
    }

    void unsubscribeFromTrack(const std::string& track_name) {
        auto sub_it = active_subscribers_.find(track_name);
        if (sub_it == active_subscribers_.end()) {
            std::cout << "Not subscribed to track: " << track_name << std::endl;
            return;
        }

        std::cout << "Unsubscribing from track: " << track_name << std::endl;

        // Stop the subscriber
        sub_it->second->stop();

        // Wait for thread to finish with timeout using async approach
        auto thread_it = subscriber_threads_.find(track_name);
        if (thread_it != subscriber_threads_.end() && thread_it->second.joinable()) {
            // Use async to wait for join with timeout
            auto join_future = std::async(std::launch::async, [&thread_it]() {
                thread_it->second.join();
            });
            
            auto status = join_future.wait_for(std::chrono::seconds(2));
            if (status == std::future_status::timeout) {
                std::cout << "Warning: Thread for track " << track_name << " taking too long to stop, detaching..." << std::endl;
                thread_it->second.detach();
            }
            
            subscriber_threads_.erase(thread_it);
        }

        // Remove subscriber
        active_subscribers_.erase(sub_it);
        std::cout << "Unsubscribed from track: " << track_name << std::endl;
    }

    void unsubscribeFromAllTracks() {
        std::cout << "Unsubscribing from all tracks..." << std::endl;
        
        for (auto& pair : active_subscribers_) {
            pair.second->stop();
        }
        
        for (auto& pair : subscriber_threads_) {
            if (pair.second.joinable()) {
                pair.second.join();
            }
        }
        
        active_subscribers_.clear();
        subscriber_threads_.clear();
        std::cout << "Unsubscribed from all tracks" << std::endl;
    }

    void showStatus() {
        std::cout << "\n=== Status ===" << std::endl;
        std::cout << "Connected: " << (is_connected_ ? "YES" : "NO") << std::endl;
        if (is_connected_) {
            std::cout << "URL: " << url_ << std::endl;
            std::cout << "Broadcast: " << broadcast_name_ << std::endl;
        }
        std::cout << "Active subscriptions: " << active_subscribers_.size() << std::endl;
        for (const auto& pair : active_subscribers_) {
            std::cout << "  - " << pair.first << ": " << pair.second->getBytesReceived() << " bytes" << std::endl;
        }
        std::cout << "=============\n" << std::endl;
    }

    void showHelp() {
        std::cout << "\n=== Keyboard Controls ===" << std::endl;
        std::cout << "c - Connect to relay" << std::endl;
        std::cout << "d - Disconnect from relay" << std::endl;
        std::cout << "v - Subscribe to video track" << std::endl;
        std::cout << "a - Subscribe to audio track" << std::endl;
        std::cout << "V - Unsubscribe from video track" << std::endl;
        std::cout << "A - Unsubscribe from audio track" << std::endl;
        std::cout << "u - Unsubscribe from all tracks" << std::endl;
        std::cout << "s - Show status" << std::endl;
        std::cout << "h - Show this help" << std::endl;
        std::cout << "q - Quit application" << std::endl;
        std::cout << "========================\n" << std::endl;
    }

    void handleKeyboardInput() {
        showHelp();
        
        while (running_) {
            if (_kbhit()) {
                char key = _getch();
                
                switch (key) {
                    case 'c':
                    case 'C':
                        connectToRelay();
                        break;
                        
                    case 'd':
                    case 'D':
                        disconnectFromRelay();
                        break;
                        
                    case 'v':
                        subscribeToTrack("video");
                        break;
                        
                    case 'a':
                        subscribeToTrack("audio");
                        break;
                        
                    case 'V':
                        unsubscribeFromTrack("video");
                        break;
                        
                    case 'A':
                        unsubscribeFromTrack("audio");
                        break;
                        
                    case 'u':
                    case 'U':
                        unsubscribeFromAllTracks();
                        break;
                        
                    case 's':
                    case 'S':
                        showStatus();
                        break;
                        
                    case 'h':
                    case 'H':
                        showHelp();
                        break;
                        
                    case 'q':
                    case 'Q':
                        std::cout << "Quitting..." << std::endl;
                        running_ = false;
                        break;
                        
                    default:
                        // Ignore other keys
                        break;
                }
            }
            
            std::this_thread::sleep_for(std::chrono::milliseconds(50));
        }
    }

    bool run() {
        // Start keyboard input handling
        handleKeyboardInput();
        return true;
    }

private:
    void stop() {
        running_ = false;
        disconnectFromRelay();
    }
};

void printUsage(const char* program_name) {
    std::cout << "Usage: " << program_name << " [OPTIONS]\n\n"
              << "Options:\n"
              << "  --url <url>          MOQ relay URL (default: https://relay2.moq.sesame-streams.com:4433)\n"
              << "  --broadcast <name>   Broadcast name to subscribe to (default: peter)\n"
              << "  --tracks <track1,track2,...>  Comma-separated list of tracks (default: video,audio)\n"
              << "  --help               Show this help message\n\n"
              << "Example:\n"
              << "  " << program_name << " --url https://relay2.moq.sesame-streams.com:4433 --broadcast peter --tracks video,audio\n"
              << std::endl;
}

std::vector<std::string> splitTracks(const std::string& tracks_str) {
    std::vector<std::string> tracks;
    std::stringstream ss(tracks_str);
    std::string track;
    
    while (std::getline(ss, track, ',')) {
        // Trim whitespace
        track.erase(0, track.find_first_not_of(" \t"));
        track.erase(track.find_last_not_of(" \t") + 1);
        if (!track.empty()) {
            tracks.push_back(track);
        }
    }
    
    return tracks;
}

int main(int argc, char* argv[]) {
    // Default values
    std::string url = "https://relay2.moq.sesame-streams.com:4433";
    std::string broadcast_name = "peter";
    std::vector<std::string> track_names = {"video", "audio"};

    // Parse command line arguments
    for (int i = 1; i < argc; i++) {
        std::string arg = argv[i];
        
        if (arg == "--url" && i + 1 < argc) {
            url = argv[++i];
        } else if (arg == "--broadcast" && i + 1 < argc) {
            broadcast_name = argv[++i];
        } else if (arg == "--tracks" && i + 1 < argc) {
            std::string tracks_str = argv[++i];
            track_names = splitTracks(tracks_str);
        } else if (arg == "--help") {
            printUsage(argv[0]);
            return 0;
        } else {
            std::cerr << "Unknown argument: " << arg << std::endl;
            printUsage(argv[0]);
            return 1;
        }
    }

    // Validate inputs
    if (url.empty()) {
        std::cerr << "Error: URL cannot be empty" << std::endl;
        return 1;
    }

    if (broadcast_name.empty()) {
        std::cerr << "Error: Broadcast name cannot be empty" << std::endl;
        return 1;
    }

    if (track_names.empty()) {
        std::cerr << "Error: At least one track must be specified" << std::endl;
        return 1;
    }

    std::cout << "MOQ Relay Test Application" << std::endl;
    std::cout << "=========================" << std::endl;
    std::cout << "URL: " << url << std::endl;
    std::cout << "Broadcast: " << broadcast_name << std::endl;
    std::cout << "Tracks: ";
    for (size_t i = 0; i < track_names.size(); ++i) {
        if (i > 0) std::cout << ", ";
        std::cout << track_names[i];
    }
    std::cout << std::endl << std::endl;

    // Create and run the test app
    RelayTestApp app(url, broadcast_name, track_names);
    
    if (!app.initialize()) {
        std::cerr << "Failed to initialize application" << std::endl;
        return 1;
    }

    if (!app.run()) {
        std::cerr << "Application failed to run" << std::endl;
        return 1;
    }

    return 0;
}