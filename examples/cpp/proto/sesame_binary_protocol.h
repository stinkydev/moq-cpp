#pragma once

#include <cstdint>
#include <cstring>
#include <vector>

namespace Sesame {
namespace Protocol {

enum class PACKET_TYPE : uint16_t {
  VIDEO_FRAME = 1,
  AUDIO_FRAME = 2,
  RPC = 3,
  MUXED_DATA = 4,
  DECODER_DATA = 5
};

enum class CODEC_TYPE : uint8_t {
  VIDEO_VP8 = 1,
  VIDEO_VP9 = 2,
  VIDEO_AVC = 3,
  VIDEO_HEVC = 4,
  VIDEO_AV1 = 5,
  AUDIO_OPUS = 64,
  AUDIO_AAC = 65,
  AUDIO_PCM = 66
};


const uint8_t FLAG_HAS_CODEC_DATA = 1 << 0;
const uint8_t FLAG_HAS_METADATA = 1 << 1;
const uint8_t FLAG_IS_KEYFRAME = 1 << 2;

const uint32_t PROTOCOL_MAGIC = 0x4D534553; // 'SESM'
const uint16_t PROTOCOL_VERSION = 1;

#pragma pack(push, 1)
struct header_data {
  uint32_t magic;         // 0x4D534553 ('SESM') - 4 bytes (offset 0)
  uint32_t flags;         // Feature flags - 2 bytes (offset 34)
  uint64_t pts;           // Presentation timestamp - 8 bytes (offset 4) 
  uint64_t id;            // Packet identifier - 8 bytes (offset 12)
  uint16_t version;       // Protocol version (currently 1) - 2 bytes (offset 28)
  uint16_t header_size;   // Total size of all headers (excluding payload) - 2 bytes (offset 30)
  PACKET_TYPE type;       // Type of packet (uint16_t) - 2 bytes (offset 32)
  uint16_t reserved;
};
#pragma pack(pop)

#pragma pack(push, 1)
struct header_codec_data {
  uint32_t sample_rate;   // Audio sample rate - 4 bytes (offset 0) - moved to front for better alignment
  uint32_t timebase_num;  // Timebase numerator - 4 bytes (offset 20)
  uint32_t timebase_den;  // Timebase denominator - 4 bytes (offset 24)
  uint16_t codec_profile; // Codec profile - 2 bytes (offset 4)
  uint16_t codec_level;   // Codec level - 2 bytes (offset 6)
  uint16_t width;         // Frame width (video only) - 2 bytes (offset 8)
  uint16_t height;        // Frame height (video only) - 2 bytes (offset 10)
  CODEC_TYPE codec_type;     // Codec identifier - 1 byte (offset 13)
  uint8_t channels;       // Audio channels - 1 byte (offset 14)
  uint8_t bit_depth;      // Bit depth (8, 10, 12, 16) - 1 byte (offset 15)
  uint8_t reserved;
};
#pragma pack(pop)

#pragma pack(push, 1)
struct header_metadata {
  char metadata[64];  // Null-terminated metadata string for routing
};
#pragma pack(pop)

// Header size constants - using constexpr for compile-time evaluation
constexpr size_t HEADER_DATA_SIZE = sizeof(header_data);
constexpr size_t HEADER_CODEC_DATA_SIZE = sizeof(header_codec_data);
constexpr size_t HEADER_METADATA_SIZE = sizeof(header_metadata);

// Helper struct for parsing incoming data
struct ParsedData {
  header_data* header;
  header_metadata* metadata;
  header_codec_data* codec_data;
  void* payload;
  size_t payload_size;
  bool valid;
};

// Helper functions for protocol serialization/deserialization
class BinaryProtocol {
public:
  // Initialize a header_data structure with proper defaults
  static void init_header(header_data* data, PACKET_TYPE type, uint32_t flags, uint64_t pts, uint64_t id);

  // Calculate the total header size based on flags
  static uint16_t calculate_header_size(uint32_t flags);
  
  // Parse incoming binary data
  static ParsedData parse_data(void* data, size_t size);
  
  // Validate the header structure
  static bool validate_header(const header_data* header, size_t total_size);
  
  // Serialize data using provided buffer (efficient, avoids allocations)
  static size_t serialize(
    void* buffer,
    size_t buffer_size,
    header_data* header,
    header_metadata* metadata,
    header_codec_data* codec_data,
    const void* payload,
    size_t payload_size
  );
};

} // namespace Protocol
} // namespace Sesame