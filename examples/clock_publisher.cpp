#include "moq_wrapper.h"

#include <chrono>
#include <iostream>
#include <string>
#include <thread>
#include <vector>

namespace
{

  void LogCallback(const std::string &target, moq::LogLevel level,
                   const std::string &message)
  {
    const char *level_str = "UNKNOWN";
    switch (level)
    {
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

  std::string GetCurrentTime()
  {
    auto now = std::chrono::system_clock::now();
    auto time_t = std::chrono::system_clock::to_time_t(now);
    auto ms = std::chrono::duration_cast<std::chrono::milliseconds>(
                  now.time_since_epoch()) %
              1000;

    std::tm tm = *std::localtime(&time_t);
    char buffer[100];
    std::strftime(buffer, sizeof(buffer), "%Y-%m-%d %H:%M:%S", &tm);

    return std::string(buffer) + "." + std::to_string(ms.count());
  }

} // namespace

int main(int argc, char *argv[])
{
  // Parse command line arguments
  std::string url = "https://relay1.moq.sesame-streams.com:4433";
  std::string broadcast = "clock-cpp";

  if (argc > 1)
  {
    url = argv[1];
  }
  if (argc > 2)
  {
    broadcast = argv[2];
  }

  std::cout << "MOQ Clock Publisher (C++)" << std::endl;
  std::cout << "Connecting to: " << url << std::endl;
  std::cout << "Broadcasting: " << broadcast << std::endl;

  // Initialize MOQ library (basic tracing only)
  moq::Init(moq::LogLevel::kInfo);

  // Define the clock track
  std::vector<moq::TrackDefinition> tracks;
  tracks.emplace_back("clock", 0, moq::TrackType::kData);

  // Create publisher session
  auto session = moq::Session::CreatePublisher(url, broadcast, tracks,
                                               moq::CatalogType::kHang);
  if (!session)
  {
    std::cerr << "Failed to create publisher session" << std::endl;
    return 1;
  }

  // Set session-specific log callback
  session->SetLogCallback(LogCallback);

  // Wait for connection
  std::cout << "Connecting..." << std::endl;
  while (!session->IsConnected())
  {
    std::this_thread::sleep_for(std::chrono::milliseconds(100));
  }
  std::cout << "Connected!" << std::endl;

  // Publish clock data every second, one group per minute
  std::cout << "Publishing clock data (press Ctrl+C to stop)..." << std::endl;
  std::cout << "Creating one group per minute with frames every second" << std::endl;

  int frame_count = 0;
  auto last_minute = std::chrono::system_clock::to_time_t(std::chrono::system_clock::now()) / 60;

  while (true)
  {
    std::string current_time = GetCurrentTime();
    auto now = std::chrono::system_clock::now();
    auto current_minute = std::chrono::system_clock::to_time_t(now) / 60;

    // Check if we've entered a new minute
    bool new_group = (current_minute != last_minute);
    if (new_group)
    {
      std::cout << "=== NEW MINUTE: Starting new group ===" << std::endl;
      last_minute = current_minute;
      frame_count = 0; // Reset frame count for new group
    }

    std::cout << "Publishing: " << current_time << " (group minute " << current_minute % 100
              << ", frame " << frame_count++ << ")" << std::endl;

    // Write frame, starting new group if it's a new minute
    if (!session->WriteFrame("clock",
                             reinterpret_cast<const uint8_t *>(current_time.c_str()),
                             current_time.length(),
                             new_group))
    {
      std::cerr << "Failed to write frame" << std::endl;
    }

    std::this_thread::sleep_for(std::chrono::seconds(1));

    // Check if still connected
    if (!session->IsConnected())
    {
      std::cout << "Connection lost. Reconnecting..." << std::endl;
      // In a real implementation, we'd handle reconnection here
      break;
    }
  }

  // Clean shutdown
  std::cout << "Shutting down..." << std::endl;
  session->Close();

  return 0;
}