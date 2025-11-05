#include "moq_wrapper.h"

#include <chrono>
#include <iostream>
#include <string>
#include <thread>
#include <vector>

namespace {

void LogCallback(const std::string& target, moq::LogLevel level,
                 const std::string& message) {
  const char* level_str = "UNKNOWN";
  switch (level) {
    case moq::LogLevel::kTrace:
      level_str = "TRACE";
      break;
    case moq::LogLevel::kDebug:
      level_str = "DEBUG";
      break;
    case moq::LogLevel::kInfo:
      level_str = "INFO";
      break;
    case moq::LogLevel::kWarn:
      level_str = "WARN";
      break;
    case moq::LogLevel::kError:
      level_str = "ERROR";
      break;
  }
  std::cout << "[" << level_str << "] " << target << ": " << message << std::endl;
}

void DataCallback(const std::string& track, const uint8_t* data, size_t size) {
  std::string data_str(reinterpret_cast<const char*>(data), size);
  auto now = std::chrono::system_clock::now();
  auto time_t = std::chrono::system_clock::to_time_t(now);
  auto ms = std::chrono::duration_cast<std::chrono::milliseconds>(
                now.time_since_epoch()) % 1000;
  
  std::tm tm = *std::localtime(&time_t);
  char buffer[100];
  std::strftime(buffer, sizeof(buffer), "%Y-%m-%d %H:%M:%S", &tm);
  
  std::string received_time = std::string(buffer) + "." + std::to_string(ms.count());
  
  std::cout << "Received on track '" << track << "' at " << received_time 
            << ": " << data_str << std::endl;
}

}  // namespace

int main(int argc, char* argv[]) {
  // Parse command line arguments
  std::string url = "https://relay.quic.video:4443";
  std::string broadcast = "clock-cpp";
  
  if (argc > 1) {
    url = argv[1];
  }
  if (argc > 2) {
    broadcast = argv[2];
  }
  
  std::cout << "MOQ Clock Subscriber (C++)" << std::endl;
  std::cout << "Connecting to: " << url << std::endl;
  std::cout << "Subscribing to: " << broadcast << std::endl;
  
  // Initialize MOQ library with custom logging
  moq::Init(moq::LogLevel::kInfo, LogCallback);
  
  // Define the tracks we want to subscribe to
  std::vector<moq::TrackDefinition> tracks;
  tracks.emplace_back("clock", 0, moq::TrackType::kData);
  
  // Create subscriber session
  auto session = moq::Session::CreateSubscriber(url, broadcast, tracks,
                                                moq::CatalogType::kHang);
  if (!session) {
    std::cerr << "Failed to create subscriber session" << std::endl;
    return 1;
  }
  
  // Set up data callback
  if (!session->SetDataCallback(DataCallback)) {
    std::cerr << "Failed to set data callback" << std::endl;
    return 1;
  }
  
  // Wait for connection
  std::cout << "Connecting..." << std::endl;
  while (!session->IsConnected()) {
    std::this_thread::sleep_for(std::chrono::milliseconds(100));
  }
  std::cout << "Connected! Waiting for data..." << std::endl;
  
  // Keep running and receiving data
  std::cout << "Listening for clock data (press Ctrl+C to stop)..." << std::endl;
  
  while (true) {
    std::this_thread::sleep_for(std::chrono::seconds(1));
    
    // Check if still connected
    if (!session->IsConnected()) {
      std::cout << "Connection lost. Attempting to reconnect..." << std::endl;
      // In a real implementation, we'd handle reconnection here
      break;
    }
  }
  
  // Clean shutdown
  std::cout << "Shutting down..." << std::endl;
  session->Close();
  
  return 0;
}