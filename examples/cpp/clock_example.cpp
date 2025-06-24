#include <moq/moq.h>
#include <iostream>
#include <chrono>
#include <thread>
#include <iomanip>
#include <sstream>
#include <future>
#include <memory>

class ClockPublisher {
private:
    std::unique_ptr<moq::TrackProducer> track_;

public:
    ClockPublisher(std::unique_ptr<moq::TrackProducer> track) 
        : track_(std::move(track)) {}

    void run() {
        auto start = std::chrono::system_clock::now();
        auto now = start;
        
        // Just for fun, don't start at zero
        auto start_time = std::chrono::system_clock::to_time_t(start);
        auto tm = *std::localtime(&start_time);
        uint64_t sequence = tm.tm_min;

        while (true) {
            // Create a new group for this minute
            auto group = track_->createGroup(sequence);
            if (!group) {
                std::cerr << "Failed to create group" << std::endl;
                break;
            }

            sequence++;

            // Send this group in a separate thread
            std::thread([group = std::move(group), now]() mutable {
                sendSegment(std::move(group), now);
            }).detach();

            // Wait until next minute
            auto next_minute = now + std::chrono::minutes(1);
            
            // Round down to start of next minute
            auto next_time_t = std::chrono::system_clock::to_time_t(next_minute);
            auto next_tm = *std::localtime(&next_time_t);
            next_tm.tm_sec = 0;
            next_tm.tm_min = (next_tm.tm_min / 1) * 1; // Ensure it's on minute boundary
            auto aligned_next = std::chrono::system_clock::from_time_t(std::mktime(&next_tm));

            auto delay = aligned_next - now;
            if (delay.count() > 0) {
                std::this_thread::sleep_for(delay);
            }

            now = aligned_next;
        }
    }

private:
    static void sendSegment(std::unique_ptr<moq::GroupProducer> group, std::chrono::system_clock::time_point start_time) {
        auto now = start_time;
        
        // Everything but the second
        auto time_t = std::chrono::system_clock::to_time_t(now);
        auto tm = *std::localtime(&time_t);
        
        std::ostringstream base_stream;
        base_stream << std::put_time(&tm, "%Y-%m-%d %H:%M:");
        std::string base = base_stream.str();
        
        // Write the base time (minute portion)
        if (!group->writeFrame(base)) {
            std::cerr << "Failed to write base frame" << std::endl;
            return;
        }

        while (true) {
            time_t = std::chrono::system_clock::to_time_t(now);
            tm = *std::localtime(&time_t);
            
            std::ostringstream delta_stream;
            delta_stream << std::setfill('0') << std::setw(2) << tm.tm_sec;
            std::string delta = delta_stream.str();
            
            // Write the seconds frame
            if (!group->writeFrame(delta)) {
                std::cerr << "Failed to write frame" << std::endl;
                break;
            }

            // Wait for next second
            auto next_second = now + std::chrono::seconds(1);
            auto next_time_t = std::chrono::system_clock::to_time_t(next_second);
            auto next_tm = *std::localtime(&next_time_t);
            auto aligned_next = std::chrono::system_clock::from_time_t(std::mktime(&next_tm));

            auto delay = aligned_next - now;
            if (delay.count() > 0) {
                std::this_thread::sleep_for(delay);
            }

            // Update time and check if we've moved to the next minute
            now = std::chrono::system_clock::now();
            auto current_time_t = std::chrono::system_clock::to_time_t(now);
            auto current_tm = *std::localtime(&current_time_t);
            auto start_time_t = std::chrono::system_clock::to_time_t(start_time);
            auto start_tm = *std::localtime(&start_time_t);
            
            if (current_tm.tm_min != start_tm.tm_min) {
                break;
            }
        }

        group->finish();
    }
};

class ClockSubscriber {
private:
    std::unique_ptr<moq::TrackConsumer> track_;

public:
    ClockSubscriber(std::unique_ptr<moq::TrackConsumer> track) 
        : track_(std::move(track)) {}

    void run() {
        while (true) {
            auto group_future = track_->nextGroup();
            auto group = group_future.get();
            
            if (!group) {
                std::cout << "No more groups available" << std::endl;
                break;
            }

            // Read the base timestamp
            auto base_future = group->readFrame();
            auto base_data = base_future.get();
            
            if (!base_data || base_data->empty()) {
                std::cout << "Empty group received" << std::endl;
                continue;
            }

            std::string base(base_data->begin(), base_data->end());

            // Read subsequent frames (seconds)
            while (true) {
                auto frame_future = group->readFrame();
                auto frame_data = frame_future.get();
                
                if (!frame_data) {
                    break; // No more frames in this group
                }

                std::string delta(frame_data->begin(), frame_data->end());
                std::cout << base << delta << std::endl;
            }
        }
    }
};

void printUsage(const char* program_name) {
    std::cout << "Usage: " << program_name << " <URL> [publish|subscribe] [options]\n"
              << "  URL: Server URL (e.g., https://moq.sesame-streams.com:4443)\n"
              << "  Mode: publish or subscribe\n"
              << "  Options:\n"
              << "    --broadcast <name>   Broadcast name (default: clock)\n"
              << "    --track <name>       Track name (default: seconds)\n"
              << "    --help               Show this help\n";
}

int main(int argc, char* argv[]) {
    if (argc < 3) {
        printUsage(argv[0]);
        return 1;
    }

    std::string url = argv[1];
    std::string mode = argv[2];
    std::string broadcast_name = "clock";
    std::string track_name = "seconds";

    // Parse additional arguments
    for (int i = 3; i < argc; i++) {
        std::string arg = argv[i];
        if (arg == "--broadcast" && i + 1 < argc) {
            broadcast_name = argv[++i];
        } else if (arg == "--track" && i + 1 < argc) {
            track_name = argv[++i];
        } else if (arg == "--help") {
            printUsage(argv[0]);
            return 0;
        }
    }

    if (mode != "publish" && mode != "subscribe") {
        std::cerr << "Error: Mode must be either 'publish' or 'subscribe'" << std::endl;
        printUsage(argv[0]);
        return 1;
    }

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
    config.bind_addr = "[::]:0";
    config.tls_disable_verify = false;

    // Create the client
    auto client = moq::Client::create(config);
    if (!client) {
        std::cerr << "Failed to create MOQ client" << std::endl;
        return 1;
    }

    std::cout << "Connecting to: " << url << std::endl;

    // Connect to the server
    auto session = client->connect(url);
    if (!session) {
        std::cerr << "Failed to connect to MOQ server" << std::endl;
        std::string error = client->getLastError();
        if (!error.empty()) {
            std::cerr << "Error: " << error << std::endl;
        }
        return 1;
    }

    std::cout << "Successfully connected to MOQ server!" << std::endl;

    // Define track information
    moq::Track track;
    track.name = track_name;
    track.priority = 0;

    if (mode == "publish") {
        std::cout << "Publishing clock to broadcast: " << broadcast_name 
                  << ", track: " << track_name << std::endl;

        // Create broadcast producer
        auto broadcast_producer = std::make_shared<moq::BroadcastProducer>();
        
        // Create track producer
        auto track_producer = broadcast_producer->createTrack(track);
        if (!track_producer) {
            std::cerr << "Failed to create track producer" << std::endl;
            return 1;
        }

        // Publish the broadcast
        if (!session->publish(broadcast_name, broadcast_producer->getConsumable())) {
            std::cerr << "Failed to publish broadcast" << std::endl;
            return 1;
        }

        // Start the clock publisher
        ClockPublisher publisher(std::move(track_producer));
        publisher.run();

    } else { // subscribe
        std::cout << "Subscribing to clock from broadcast: " << broadcast_name 
                  << ", track: " << track_name << std::endl;

        // Consume the broadcast
        auto broadcast_consumer = session->consume(broadcast_name);
        if (!broadcast_consumer) {
            std::cerr << "Failed to consume broadcast" << std::endl;
            return 1;
        }

        // Subscribe to the track
        auto track_consumer = broadcast_consumer->subscribeTrack(track);
        if (!track_consumer) {
            std::cerr << "Failed to subscribe to track" << std::endl;
            return 1;
        }

        // Start the clock subscriber
        ClockSubscriber subscriber(std::move(track_consumer));
        subscriber.run();
    }

    return 0;
}
