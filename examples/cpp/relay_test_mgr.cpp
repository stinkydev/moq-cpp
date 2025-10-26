#include <moq-mgr/session.h>
#include <moq-mgr/consumer.h>
#include "proto/sesame_binary_protocol.h"
#include <iostream>
#include <chrono>
#include <thread>
#include <atomic>
#include <memory>
#include <future>
#include <iomanip>
#include <sstream>
#include <map>
#include <cstring>
#include <conio.h>  // For _kbhit() and _getch() on Windows

class TrackDataHandler {
private:
    std::string track_name_;
    std::atomic<uint64_t> bytes_received_;
    std::atomic<uint64_t> groups_received_;
    std::atomic<uint64_t> keyframes_received_;
    std::chrono::system_clock::time_point start_time_;
    bool parse_protocol_;

public:
    explicit TrackDataHandler(const std::string& track_name, bool parse_protocol = false) 
        : track_name_(track_name), bytes_received_(0), groups_received_(0),
          keyframes_received_(0), start_time_(std::chrono::system_clock::now()),
          parse_protocol_(parse_protocol) {}

    void handleData(uint8_t* data, size_t size) {
        bytes_received_ += size;
        groups_received_++;

        bool is_keyframe = false;
        std::string packet_info;
        
        if (parse_protocol_) {
            // Parse packet using Sesame Binary Protocol
            auto parsed = Sesame::Protocol::BinaryProtocol::parse_data(data, size);
        
            if (parsed.valid) {
                is_keyframe = (parsed.header->flags & Sesame::Protocol::FLAG_IS_KEYFRAME) != 0;
            
                if (is_keyframe) {
                    keyframes_received_++;
                }
            
                // Build detailed packet info
                std::stringstream ss;
                ss << " [";
            
                // Packet type
                switch (parsed.header->type) {
                    case Sesame::Protocol::PACKET_TYPE::VIDEO_FRAME:
                        ss << "VIDEO";
                        break;
                    case Sesame::Protocol::PACKET_TYPE::AUDIO_FRAME:
                        ss << "AUDIO";
                        break;
                    case Sesame::Protocol::PACKET_TYPE::RPC:
                        ss << "RPC";
                        break;
                    case Sesame::Protocol::PACKET_TYPE::MUXED_DATA:
                        ss << "MUXED";
                        break;
                    case Sesame::Protocol::PACKET_TYPE::DECODER_DATA:
                        ss << "DECODER";
                        break;
                    default:
                        ss << "UNKNOWN";
                        break;
                }
            
                // Keyframe status
                ss << (is_keyframe ? ", key" : "");
            
                // PTS
                ss << ", PTS:" << parsed.header->pts;
            
                // Codec info if available
                if (parsed.codec_data) {
                    ss << ", ";
                    switch (parsed.codec_data->codec_type) {
                        case Sesame::Protocol::CODEC_TYPE::VIDEO_VP8:
                            ss << "VP8";
                            break;
                        case Sesame::Protocol::CODEC_TYPE::VIDEO_VP9:
                            ss << "VP9";
                            break;
                        case Sesame::Protocol::CODEC_TYPE::VIDEO_AVC:
                            ss << "AVC";
                            break;
                        case Sesame::Protocol::CODEC_TYPE::VIDEO_HEVC:
                            ss << "HEVC";
                            break;
                        case Sesame::Protocol::CODEC_TYPE::VIDEO_AV1:
                            ss << "AV1";
                            break;
                        case Sesame::Protocol::CODEC_TYPE::AUDIO_OPUS:
                            ss << "OPUS";
                            break;
                        case Sesame::Protocol::CODEC_TYPE::AUDIO_AAC:
                            ss << "AAC";
                            break;
                        case Sesame::Protocol::CODEC_TYPE::AUDIO_PCM:
                            ss << "PCM";
                            break;
                        default:
                            ss << "UNKNOWN_CODEC";
                            break;
                    }
                
                    // Add resolution for video
                    if (parsed.header->type == Sesame::Protocol::PACKET_TYPE::VIDEO_FRAME) {
                        ss << " " << parsed.codec_data->width << "x" << parsed.codec_data->height;
                    }
                
                    // Add sample rate for audio
                    if (parsed.header->type == Sesame::Protocol::PACKET_TYPE::AUDIO_FRAME) {
                        ss << " " << parsed.codec_data->sample_rate << " hz";
                    }
                }
            
                // Payload info with first and last bytes
                ss << ", payload:" << parsed.payload_size;
                if (parsed.payload_size > 0) {
                    const uint8_t* payload_bytes = static_cast<const uint8_t*>(parsed.payload);
                    ss << " [0x" << std::hex << std::setfill('0') << std::setw(2) 
                       << static_cast<int>(payload_bytes[0]);
                    if (parsed.payload_size > 1) {
                        ss << "...0x" << std::setw(2) 
                           << static_cast<int>(payload_bytes[parsed.payload_size - 1]);
                    }
                    ss << std::dec << "]";
                }
            
                ss << "]";
                packet_info = ss.str();
            } else {
                packet_info = " [INVALID PACKET]";
            }
        } else {
            // Simple raw data logging when protocol parsing is disabled
            std::stringstream ss;
            ss << " [RAW DATA";
            if (size > 0) {
                ss << ", first:0x" << std::hex << std::setfill('0') << std::setw(2)
                   << static_cast<int>(data[0]);
                if (size > 1) {
                    ss << ", last:0x" << std::setw(2) 
                       << static_cast<int>(data[size - 1]);
                }
                ss << std::dec;
            }
            ss << "]";
            packet_info = ss.str();
        }
        
        // Log packet information
        std::cout << "Track " << track_name_ << ": Size " 
                  << size << " bytes" << packet_info << std::endl;
        
        // Log every 100 groups or 1MB of data
        if (groups_received_ % 100 == 0 || bytes_received_ % (1024*1024) == 0) {
            auto now = std::chrono::system_clock::now();
            auto duration = std::chrono::duration_cast<std::chrono::seconds>(now - start_time_).count();
            
            std::cout << "Track " << track_name_ << ": " << groups_received_ 
                      << " groups, " << keyframes_received_ << " keyframes, "
                      << bytes_received_ << " bytes"
                      << " (avg " << (bytes_received_ / std::max(duration, static_cast<decltype(duration)>(1))) << " B/s)" << std::endl;
        }
    }

    uint64_t getBytesReceived() const {
        return bytes_received_;
    }

    uint64_t getGroupsReceived() const {
        return groups_received_;
    }
    
    uint64_t getKeyframesReceived() const {
        return keyframes_received_;
    }

    const std::string& getTrackName() const {
        return track_name_;
    }
};

class RelayTestMgrApp {
private:
    std::string url_;
    std::string broadcast_name_;
    std::vector<std::string> available_track_names_;
    std::atomic<bool> running_;
    bool parse_protocol_;
    
    // MOQ MGR objects
    std::unique_ptr<moq_mgr::ConsumerSession> consumer_session_;
    std::map<std::string, std::shared_ptr<TrackDataHandler>> track_handlers_;
    bool is_connected_;

public:
    RelayTestMgrApp(const std::string& url, const std::string& broadcast_name, 
                   const std::vector<std::string>& track_names, bool parse_protocol = false)
        : url_(url), broadcast_name_(broadcast_name), available_track_names_(track_names), 
          running_(true), parse_protocol_(parse_protocol), is_connected_(false) {}

    ~RelayTestMgrApp() {
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

        std::cout << "Connecting to: " << url_ << std::endl;

        // Create session configuration
        moq_mgr::Session::SessionConfig config;
        config.moq_server = url_;
        config.moq_namespace = broadcast_name_;

        // Create subscription configurations for available tracks
        std::vector<moq_mgr::Consumer::SubscriptionConfig> subscriptions;
        
        for (const auto& track_name : available_track_names_) {
            // Create track data handler with protocol parsing option
            auto handler = std::make_shared<TrackDataHandler>(track_name, parse_protocol_);
            track_handlers_[track_name] = handler;
            
            // Create subscription config
            moq_mgr::Consumer::SubscriptionConfig sub_config;
            sub_config.moq_track_name = track_name;
            sub_config.data_callback = [handler](uint8_t* data, size_t size) {
                handler->handleData(data, size);
            };
            
            subscriptions.push_back(sub_config);
        }

        // Create consumer session
        consumer_session_ = std::make_unique<moq_mgr::ConsumerSession>(config, std::move(subscriptions));
        
        // Set up callbacks
        consumer_session_->set_error_callback([](const std::string& error) {
            std::cerr << "Session error: " << error << std::endl;
        });
        
        consumer_session_->set_status_callback([](const std::string& status) {
            std::cout << "Session status: " << status << std::endl;
        });

        // Start the session
        if (!consumer_session_->start()) {
            std::cerr << "Failed to start consumer session" << std::endl;
            consumer_session_.reset();
            return false;
        }

        std::cout << "Successfully connected to MOQ server" << std::endl;
        is_connected_ = true;
        return true;
    }

    void disconnectFromRelay() {
        if (!is_connected_) {
            std::cout << "Not connected to relay" << std::endl;
            return;
        }

        std::cout << "Disconnecting from relay..." << std::endl;
        
        // Stop the consumer session
        if (consumer_session_) {
            consumer_session_->stop();
            consumer_session_.reset();
        }
        
        // Clear track handlers
        track_handlers_.clear();
        
        is_connected_ = false;
        std::cout << "Disconnected from relay" << std::endl;
    }

    void showStatus() {
        std::cout << "\n=== Status ===" << std::endl;
        std::cout << "Connected: " << (is_connected_ ? "YES" : "NO") << std::endl;
        if (is_connected_) {
            std::cout << "URL: " << url_ << std::endl;
            std::cout << "Broadcast: " << broadcast_name_ << std::endl;
            std::cout << "Session Running: " << (consumer_session_ && consumer_session_->is_running() ? "YES" : "NO") << std::endl;
        }
        std::cout << "Active tracks: " << track_handlers_.size() << std::endl;
        for (const auto& pair : track_handlers_) {
            std::cout << "  - " << pair.first << ": " 
                      << pair.second->getGroupsReceived() << " groups, "
                      << pair.second->getKeyframesReceived() << " keyframes, "
                      << pair.second->getBytesReceived() << " bytes" << std::endl;
        }
        std::cout << "=============\n" << std::endl;
    }

    void showHelp() {
        std::cout << "\n=== Keyboard Controls ===" << std::endl;
        std::cout << "c - Connect to relay (automatically subscribes to all configured tracks)" << std::endl;
        std::cout << "d - Disconnect from relay" << std::endl;
        std::cout << "s - Show status" << std::endl;
        std::cout << "h - Show this help" << std::endl;
        std::cout << "q - Quit application" << std::endl;
        std::cout << "\nNote: With MOQ Manager, all tracks are subscribed automatically when connecting." << std::endl;
        std::cout << "Track subscriptions: ";
        for (size_t i = 0; i < available_track_names_.size(); ++i) {
            if (i > 0) std::cout << ", ";
            std::cout << available_track_names_[i];
        }
        std::cout << std::endl;
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
              << "  --url <url>          MOQ relay URL (default: https://relay1.moq.sesame-streams.com:4433)\n"
              << "  --broadcast <name>   Broadcast name to subscribe to (default: peter)\n"
              << "  --tracks <track1,track2,...>  Comma-separated list of tracks (default: video,audio)\n"
              << "  --parse-protocol     Enable Sesame Binary Protocol parsing (default: off)\n"
              << "  --help               Show this help message\n\n"
              << "Example:\n"
              << "  " << program_name << " --url https://relay1.moq.sesame-streams.com:4433 --broadcast peter --tracks video,audio\n"
              << "  " << program_name << " --broadcast peter --parse-protocol\n\n"
              << "This example uses the MOQ Manager abstraction which automatically handles session management,\n"
              << "reconnection, and subscription lifecycle. All configured tracks are subscribed when connecting.\n"
              << "Use --parse-protocol to enable detailed parsing of Sesame Binary Protocol packets.\n"
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
    std::string url = "https://relay1.moq.sesame-streams.com:4433";
    std::string broadcast_name = "peter";
    std::vector<std::string> track_names = {"video", "audio"};
    bool parse_protocol = false;

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
        } else if (arg == "--parse-protocol") {
            parse_protocol = true;
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

    std::cout << "MOQ Relay Test Application (using MOQ Manager)" << std::endl;
    std::cout << "=============================================" << std::endl;
    std::cout << "URL: " << url << std::endl;
    std::cout << "Broadcast: " << broadcast_name << std::endl;
    std::cout << "Tracks: ";
    for (size_t i = 0; i < track_names.size(); ++i) {
        if (i > 0) std::cout << ", ";
        std::cout << track_names[i];
    }
    std::cout << std::endl;
    std::cout << "Protocol Parsing: " << (parse_protocol ? "ENABLED" : "DISABLED") << std::endl;
    std::cout << std::endl;

    // Create and run the test app
    RelayTestMgrApp app(url, broadcast_name, track_names, parse_protocol);
    
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