#include "moq_wrapper.h"

#include <atomic>
#include <chrono>
#include <iostream>
#include <map>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

namespace
{

constexpr const char *kDefaultRelay = "https://r2.moq.sesame-streams.com:4433";
constexpr const char *kDefaultBroadcast = "clock-cpp";
constexpr const char *kTrackName = "clock";

moq::CatalogType ParseCatalogType(const std::string &value)
{
  if (value == "sesame")
  {
    return moq::CatalogType::kSesame;
  }
  if (value == "hang")
  {
    return moq::CatalogType::kHang;
  }
  return moq::CatalogType::kNone;
}

bool ParseBool(const std::string &value)
{
  return value == "1" || value == "true" || value == "yes" ||
         value == "all" || value == "all-catalog-tracks";
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

void PrintUsage(const char *program)
{
  std::cout << "Usage: " << program
            << " [url] [broadcast_or_room_prefix] [exact|room] [track] [none|sesame|hang] [all_catalog_tracks]"
            << std::endl;
}

} // namespace

int main(int argc, char *argv[])
{
  moq::SetLogLevel(moq::LogLevel::kInfo);

  const std::string url = argc > 1 ? argv[1] : kDefaultRelay;
  const std::string target = argc > 2 ? argv[2] : kDefaultBroadcast;
  const std::string mode = argc > 3 ? argv[3] : "exact";
  const std::string track_name = argc > 4 ? argv[4] : kTrackName;
  const bool all_catalog_tracks = argc > 6 ? ParseBool(argv[6]) : false;
  moq::CatalogType catalog_type = argc > 5 ? ParseCatalogType(argv[5]) : moq::CatalogType::kNone;
  if (all_catalog_tracks && catalog_type == moq::CatalogType::kNone)
  {
    catalog_type = moq::CatalogType::kSesame;
  }

  if (mode != "exact" && mode != "room")
  {
    PrintUsage(argv[0]);
    return 1;
  }

  std::vector<moq::TrackDefinition> tracks;
  if (!all_catalog_tracks)
  {
    tracks.emplace_back(track_name, 0, moq::TrackType::kData);
  }

  std::cout << "MoQ C++ clock subscriber" << std::endl;
  std::cout << "Relay: " << url << std::endl;
  std::cout << (mode == "room" ? "Room prefix: " : "Broadcast: ") << target << std::endl;
  std::cout << "Track mode: "
            << (all_catalog_tracks ? "all catalog tracks" : track_name)
            << std::endl;

  auto session = mode == "room"
                     ? moq::Session::CreateRoomSubscriber(
                           url, target, tracks, catalog_type, all_catalog_tracks)
                     : moq::Session::CreateSubscriber(
                           url, target, tracks, catalog_type, all_catalog_tracks);

  if (!session)
  {
    std::cerr << "Failed to create subscriber session" << std::endl;
    return 1;
  }

  std::mutex counters_mutex;
  std::map<std::string, size_t> counters;

  session->SetDataCallback(
      [&counters_mutex, &counters](const std::string &track,
                                   const uint8_t *data, size_t size)
      {
        const std::string payload(reinterpret_cast<const char *>(data), size);
        size_t count = 0;
        {
          std::lock_guard<std::mutex> lock(counters_mutex);
          count = ++counters[track];
        }

        std::cout << "Frame #" << count << " on '" << track << "': "
                  << payload << std::endl;
      });

  session->SetBroadcastAnnouncedCallback(
      [](const std::string &path)
      {
        std::cout << "Broadcast announced: " << path << std::endl;
      });

  session->SetBroadcastCancelledCallback(
      [](const std::string &path)
      {
        std::cout << "Broadcast cancelled: " << path << std::endl;
      });

  session->SetConnectionClosedCallback(
      [](const std::string &reason)
      {
        std::cerr << "Connection closed: " << reason << std::endl;
      });

  std::atomic<bool> should_stop{false};
  if (!WaitForConnection(*session, should_stop))
  {
    std::cerr << "Timed out waiting for connection" << std::endl;
    session->Close();
    return 1;
  }

  std::cout << "Connected. Press Enter to stop." << std::endl;
  std::cin.get();
  should_stop = true;
  session->Close();

  return 0;
}
