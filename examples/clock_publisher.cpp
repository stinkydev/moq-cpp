#include "moq_wrapper.h"

#include <atomic>
#include <chrono>
#include <iostream>
#include <memory>
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

  // New callback functions for broadcast events
  void BroadcastAnnouncedCallback(const std::string &path)
  {
    std::cout << "🟢 BROADCAST ANNOUNCED: " << path << std::endl;
  }

  void BroadcastCancelledCallback(const std::string &path)
  {
    std::cout << "🔴 BROADCAST CANCELLED: " << path << std::endl;
  }

  void ConnectionClosedCallback(const std::string &reason)
  {
    std::cout << "❌ CONNECTION CLOSED: " << reason << std::endl;
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

// Session management thread function
void SessionManagerThread(const std::string &url, const std::string &broadcast,
                          std::shared_ptr<moq::Session> &session, std::atomic<bool> &session_ready,
                          std::atomic<bool> &should_stop)
{
  // Define the clock track
  std::vector<moq::TrackDefinition> tracks;
  tracks.emplace_back("clock", 0, moq::TrackType::kData);

  std::cout << "[SESSION] Creating publisher session..." << std::endl;
  session = moq::Session::CreatePublisher(url, broadcast, tracks, moq::CatalogType::kSesame);

  if (!session)
  {
    std::cerr << "[SESSION] Failed to create publisher session" << std::endl;
    should_stop = true;
    return;
  }

  // Set session-specific log callback
  session->SetLogCallback(LogCallback);

  // Set up the new broadcast event callbacks
  std::cout << "[SESSION] Setting up broadcast event callbacks..." << std::endl;
  if (!session->SetBroadcastAnnouncedCallback(BroadcastAnnouncedCallback))
  {
    std::cerr << "[SESSION] Failed to set broadcast announced callback" << std::endl;
  }

  if (!session->SetBroadcastCancelledCallback(BroadcastCancelledCallback))
  {
    std::cerr << "[SESSION] Failed to set broadcast cancelled callback" << std::endl;
  }

  if (!session->SetConnectionClosedCallback(ConnectionClosedCallback))
  {
    std::cerr << "[SESSION] Failed to set connection closed callback" << std::endl;
  }

  std::cout << "[SESSION] All callbacks configured successfully" << std::endl;

  // Wait for connection
  std::cout << "[SESSION] Connecting..." << std::endl;
  while (!session->IsConnected() && !should_stop)
  {
    std::this_thread::sleep_for(std::chrono::milliseconds(100));
  }

  if (!should_stop)
  {
    std::cout << "[SESSION] Connected!" << std::endl;
    session_ready = true;
  }

  // Keep session alive and monitor connection
  while (!should_stop)
  {
    std::this_thread::sleep_for(std::chrono::milliseconds(500));

    if (!session->IsConnected())
    {
      if (session_ready)
      {
        std::cout << "[SESSION] Connection lost! Waiting for reconnection..." << std::endl;
        session_ready = false;
      }
      // Don't break - let the session attempt to reconnect
      // The Rust layer handles automatic reconnection
    }
    else
    {
      if (!session_ready)
      {
        std::cout << "[SESSION] Reconnected!" << std::endl;
        session_ready = true;
      }
    }
  }

  std::cout << "[SESSION] Shutting down session..." << std::endl;
  if (session)
  {
    session->Close();
  }
}

// Data publishing thread function
void DataPublishThread(std::shared_ptr<moq::Session> &session,
                       std::atomic<bool> &session_ready, std::atomic<bool> &should_stop)
{
  std::cout << "[DATA] Waiting for session to be ready..." << std::endl;

  // Wait for session to be ready
  while (!session_ready && !should_stop)
  {
    std::this_thread::sleep_for(std::chrono::milliseconds(100));
  }

  if (should_stop)
  {
    return;
  }

  std::cout << "[DATA] Session ready, waiting for track producers to be created..." << std::endl;

  // Give additional time for track producers to be created asynchronously
  // This prevents the "Track producer not available" race condition
  std::this_thread::sleep_for(std::chrono::milliseconds(1000));

  std::cout << "[DATA] Starting data publishing..." << std::endl;
  std::cout << "[DATA] Publishing clock data (creating one group per minute with frames every second)" << std::endl;

  int frame_count = 0;
  auto last_minute = std::chrono::system_clock::to_time_t(std::chrono::system_clock::now()) / 60;

  while (!should_stop && session_ready)
  {
    std::string current_time = GetCurrentTime();
    auto now = std::chrono::system_clock::now();
    auto current_minute = std::chrono::system_clock::to_time_t(now) / 60;

    bool new_group = (current_minute != last_minute);
    if (new_group)
    {
      std::cout << "[DATA] === NEW MINUTE: Starting new group ===" << std::endl;
      last_minute = current_minute;
      frame_count = 0; // Reset frame count for new group
    }

    std::cout << "[DATA] Publishing: " << current_time << " (group minute " << current_minute % 100
              << ", frame " << frame_count++ << ")" << std::endl;

    // Write frame, starting new group if it's a new minute
    if (session && session_ready)
    {
      if (!session->WriteFrame("clock",
                               reinterpret_cast<const uint8_t *>(current_time.c_str()),
                               current_time.length(),
                               new_group))
      {
        std::cerr << "[DATA] Failed to write frame (connection may be down)" << std::endl;
      }
    }
    else if (session && !session_ready)
    {
      std::cout << "[DATA] Waiting for connection to be ready..." << std::endl;
    }

    // std::this_thread::sleep_for(std::chrono::seconds(1));
    std::this_thread::sleep_for(std::chrono::milliseconds(20));
  }

  std::cout << "[DATA] Data publishing thread stopping..." << std::endl;
}

int main(int argc, char *argv[])
{
  // Set up global logging for library diagnostics (optional)
  moq::SetLogLevel(moq::LogLevel::kDebug);

  // Parse command line arguments
  std::string url = "https://r1.moq.sesame-streams.com:4433";
  std::string broadcast = "clock-cpp";

  if (argc > 1)
  {
    url = argv[1];
  }
  if (argc > 2)
  {
    broadcast = argv[2];
  }

  std::cout << "MOQ Clock Publisher (C++) - Multi-threaded Version" << std::endl;
  std::cout << "Connecting to: " << url << std::endl;
  std::cout << "Broadcasting: " << broadcast << std::endl;

  // Shared state between threads
  std::shared_ptr<moq::Session> session;
  std::atomic<bool> session_ready{false};
  std::atomic<bool> should_stop{false};

  // Start session management thread
  std::thread session_thread(SessionManagerThread, std::ref(url), std::ref(broadcast),
                             std::ref(session), std::ref(session_ready), std::ref(should_stop));

  // Start data publishing thread
  std::thread data_thread(DataPublishThread, std::ref(session),
                          std::ref(session_ready), std::ref(should_stop));

  std::cout << "Press Enter to stop..." << std::endl;
  std::cin.get();

  // Signal threads to stop
  should_stop = true;

  // Wait for threads to complete
  if (session_thread.joinable())
  {
    session_thread.join();
  }
  if (data_thread.joinable())
  {
    data_thread.join();
  }

  std::cout << "Application shutdown complete." << std::endl;
  return 0;
}