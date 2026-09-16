#include "moq_wrapper.h"

#include <algorithm>
#include <atomic>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <ctime>
#include <iostream>
#include <string>
#include <thread>
#include <vector>

namespace
{

constexpr const char *kDefaultRelay = "https://r2.moq.sesame-streams.com:4433";
constexpr const char *kDefaultBroadcast = "clock-cpp";
constexpr const char *kTrackName = "clock";

std::string NowString()
{
  const auto now = std::chrono::system_clock::now();
  const auto millis = std::chrono::duration_cast<std::chrono::milliseconds>(
                          now.time_since_epoch()) %
                      1000;
  const std::time_t now_time = std::chrono::system_clock::to_time_t(now);

  std::tm local_time{};
#ifdef _WIN32
  localtime_s(&local_time, &now_time);
#else
  localtime_r(&now_time, &local_time);
#endif

  char date_time[32];
  std::strftime(date_time, sizeof(date_time), "%Y-%m-%dT%H:%M:%S", &local_time);

  char result[64];
  std::snprintf(result, sizeof(result), "%s.%03lld", date_time,
                static_cast<long long>(millis.count()));
  return result;
}

size_t ParseSize(const char *value, size_t fallback)
{
  if (!value)
  {
    return fallback;
  }

  char *end = nullptr;
  const unsigned long parsed = std::strtoul(value, &end, 10);
  if (end == value || parsed == 0)
  {
    return fallback;
  }

  return static_cast<size_t>(parsed);
}

std::string BroadcastForIndex(const std::string &base, size_t index,
                              size_t publisher_count)
{
  if (publisher_count == 1)
  {
    return base;
  }

  std::string prefix = base;
  while (!prefix.empty() && prefix.back() == '/')
  {
    prefix.pop_back();
  }

  return prefix + "/publisher-" + std::to_string(index + 1);
}

bool WaitForConnection(const moq::Session &session, std::atomic<bool> &should_stop)
{
  const auto start = std::chrono::steady_clock::now();
  while (!should_stop && !session.IsConnected())
  {
    if (std::chrono::steady_clock::now() - start > std::chrono::seconds(10))
    {
      return false;
    }
    std::this_thread::sleep_for(std::chrono::milliseconds(100));
  }

  return !should_stop && session.IsConnected();
}

void PublisherThread(const std::string &url, const std::string &broadcast,
                     size_t interval_ms, std::atomic<bool> &should_stop)
{
  std::vector<moq::TrackDefinition> tracks;
  tracks.emplace_back(kTrackName, 0, moq::TrackType::kData);

  auto session =
      moq::Session::CreatePublisher(url, broadcast, tracks, moq::CatalogType::kNone);
  if (!session)
  {
    std::cerr << "[" << broadcast << "] failed to create publisher session" << std::endl;
    should_stop = true;
    return;
  }

  session->SetConnectionClosedCallback(
      [broadcast](const std::string &reason)
      {
        std::cerr << "[" << broadcast << "] connection closed: " << reason << std::endl;
      });

  if (!WaitForConnection(*session, should_stop))
  {
    std::cerr << "[" << broadcast << "] timed out waiting for connection" << std::endl;
    session->Close();
    return;
  }

  std::cout << "[" << broadcast << "] connected and publishing track '"
            << kTrackName << "'" << std::endl;

  const auto interval = std::chrono::milliseconds(std::max<size_t>(interval_ms, 1));
  while (!should_stop)
  {
    const std::string payload = broadcast + " " + NowString();
    const auto *data = reinterpret_cast<const uint8_t *>(payload.data());

    if (!session->WriteSingleFrame(kTrackName, data, payload.size()))
    {
      std::cerr << "[" << broadcast << "] failed to publish frame" << std::endl;
    }
    else
    {
      std::cout << "[" << broadcast << "] " << payload << std::endl;
    }

    std::this_thread::sleep_for(interval);
  }

  session->Close();
}

} // namespace

int main(int argc, char *argv[])
{
  moq::SetLogLevel(moq::LogLevel::kInfo);

  const std::string url = argc > 1 ? argv[1] : kDefaultRelay;
  const std::string broadcast = argc > 2 ? argv[2] : kDefaultBroadcast;
  const size_t publisher_count = argc > 3 ? ParseSize(argv[3], 1) : 1;
  const size_t interval_ms = argc > 4 ? ParseSize(argv[4], 1000) : 1000;

  std::cout << "MoQ C++ clock publisher" << std::endl;
  std::cout << "Relay: " << url << std::endl;
  std::cout << "Broadcast base: " << broadcast << std::endl;
  std::cout << "Publishers: " << publisher_count << std::endl;
  std::cout << "Interval: " << interval_ms << " ms" << std::endl;

  std::atomic<bool> should_stop{false};
  std::vector<std::thread> threads;
  threads.reserve(publisher_count);

  for (size_t i = 0; i < publisher_count; ++i)
  {
    threads.emplace_back(PublisherThread, url,
                         BroadcastForIndex(broadcast, i, publisher_count),
                         interval_ms, std::ref(should_stop));
  }

  std::cout << "Press Enter to stop." << std::endl;
  std::cin.get();
  should_stop = true;

  for (auto &thread : threads)
  {
    if (thread.joinable())
    {
      thread.join();
    }
  }

  return 0;
}
