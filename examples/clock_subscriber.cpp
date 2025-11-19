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

  void DataCallback(const std::string &track, const uint8_t *data, size_t size)
  {
    std::string data_str(reinterpret_cast<const char *>(data), size);
    auto now = std::chrono::system_clock::now();
    auto time_t = std::chrono::system_clock::to_time_t(now);
    auto ms = std::chrono::duration_cast<std::chrono::milliseconds>(
                  now.time_since_epoch()) %
              1000;

    std::tm tm = *std::localtime(&time_t);
    char buffer[100];
    std::strftime(buffer, sizeof(buffer), "%Y-%m-%d %H:%M:%S", &tm);

    std::string received_time = std::string(buffer) + "." + std::to_string(ms.count());

    std::cout << "Received on track '" << track << "' at " << received_time
              << ": " << data_str << std::endl;
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

} // namespace

// Session management thread function
void SessionManagerThread(const std::string &url, const std::string &broadcast,
                          std::shared_ptr<moq::Session> &session, std::atomic<bool> &session_ready,
                          std::atomic<bool> &should_stop)
{
  // Define the tracks we want to subscribe to
  std::vector<moq::TrackDefinition> tracks;
  tracks.emplace_back("clock", 0, moq::TrackType::kData);
  tracks.emplace_back("clock2", 0, moq::TrackType::kData);

  std::cout << "[SESSION] Created track definitions:" << std::endl;
  for (size_t i = 0; i < tracks.size(); ++i)
  {
    std::cout << "  [" << i << "] '" << tracks[i].name() << "'" << std::endl;
  }

  std::cout << "[SESSION] Creating subscriber session..." << std::endl;
  session = moq::Session::CreateSubscriber(url, broadcast, tracks, moq::CatalogType::kSesame);

  if (!session)
  {
    std::cerr << "[SESSION] Failed to create subscriber session" << std::endl;
    should_stop = true;
    return;
  }

  // Set session-specific log callback
  session->SetLogCallback(LogCallback);

  // Set up data callback
  if (!session->SetDataCallback(DataCallback))
  {
    std::cerr << "[SESSION] Failed to set data callback" << std::endl;
    should_stop = true;
    return;
  }

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
    std::cout << "[SESSION] Connected! Waiting for data..." << std::endl;
    session_ready = true;
  }

  // Keep session alive and monitor connection
  while (!should_stop)
  {
    std::this_thread::sleep_for(std::chrono::milliseconds(500));

    if (session && !session->IsConnected())
    {
      std::cout << "[SESSION] Connection lost!" << std::endl;
      session_ready = false;
      // In a real implementation, we'd handle reconnection here
      break;
    }
  }

  std::cout << "[SESSION] Shutting down session..." << std::endl;
  if (session)
  {
    session->Close();
  }
}

// Data monitoring thread function
void DataMonitorThread(std::shared_ptr<moq::Session> &session,
                       std::atomic<bool> &session_ready, std::atomic<bool> &should_stop)
{
  std::cout << "[MONITOR] Waiting for session to be ready..." << std::endl;

  // Wait for session to be ready
  while (!session_ready && !should_stop)
  {
    std::this_thread::sleep_for(std::chrono::milliseconds(100));
  }

  if (should_stop)
  {
    return;
  }

  std::cout << "[MONITOR] Session ready, monitoring for data..." << std::endl;
  std::cout << "[MONITOR] Listening for clock data..." << std::endl;

  // Monitor session status and provide periodic updates
  auto last_status_update = std::chrono::steady_clock::now();

  while (!should_stop && session_ready)
  {
    std::this_thread::sleep_for(std::chrono::seconds(1));

    auto now = std::chrono::steady_clock::now();
    auto elapsed = std::chrono::duration_cast<std::chrono::seconds>(now - last_status_update).count();

    // Print status update every 30 seconds
    if (elapsed >= 30)
    {
      if (session && session->IsConnected())
      {
        std::cout << "[MONITOR] Status: Connected and listening..." << std::endl;
      }
      else
      {
        std::cout << "[MONITOR] Status: Disconnected" << std::endl;
        session_ready = false;
      }
      last_status_update = now;
    }
  }

  std::cout << "[MONITOR] Data monitoring thread stopping..." << std::endl;
}

int main(int argc, char *argv[])
{
  // Set up global logging for library diagnostics (optional)
  moq::SetLogLevel(moq::LogLevel::kInfo);

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

  std::cout << "MOQ Clock Subscriber (C++) - With Event Callbacks (No Reconnection)" << std::endl;
  std::cout << "Connecting to: " << url << std::endl;
  std::cout << "Subscribing to: " << broadcast << std::endl;
  std::cout << std::endl;
  std::cout << "New Features:" << std::endl;
  std::cout << "• 🟢 Broadcast Announced callbacks - when a broadcast becomes active" << std::endl;
  std::cout << "• 🔴 Broadcast Cancelled callbacks - when a broadcast is stopped" << std::endl;
  std::cout << "• ❌ Connection Closed callbacks - when connection ends (no auto-reconnect)" << std::endl;
  std::cout << std::endl;

  // Shared state between threads
  std::shared_ptr<moq::Session> session;
  std::atomic<bool> session_ready{false};
  std::atomic<bool> should_stop{false};

  // Start session management thread
  std::thread session_thread(SessionManagerThread, std::ref(url), std::ref(broadcast),
                             std::ref(session), std::ref(session_ready), std::ref(should_stop));

  // Start data monitoring thread
  std::thread monitor_thread(DataMonitorThread, std::ref(session),
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
  if (monitor_thread.joinable())
  {
    monitor_thread.join();
  }

  std::cout << "Application shutdown complete." << std::endl;
  return 0;
}