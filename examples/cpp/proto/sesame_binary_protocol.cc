#include "sesame_binary_protocol.h"
#include <algorithm>

namespace Sesame {
namespace Protocol {

void BinaryProtocol::init_header(header_data* data, PACKET_TYPE type, uint32_t flags, uint64_t pts, uint64_t id) {
    if (!data) return;
    
    data->magic = PROTOCOL_MAGIC;
    data->version = PROTOCOL_VERSION;
    data->header_size = calculate_header_size(flags);
    data->type = type;
    data->flags = flags;
    data->pts = pts;
    data->id = id;
    data->reserved = 0; // Always zero reserved fields
}

uint16_t BinaryProtocol::calculate_header_size(uint32_t flags) {
    uint16_t size = HEADER_DATA_SIZE;
    
    if (flags & FLAG_HAS_METADATA) {
        size += HEADER_METADATA_SIZE;
    }
    
    if (flags & FLAG_HAS_CODEC_DATA) {
        size += HEADER_CODEC_DATA_SIZE;
    }
    
    return size;
}

ParsedData BinaryProtocol::parse_data(void* data, size_t size) {
    ParsedData result = {};
    result.valid = false;
    
    if (!data || size < sizeof(header_data)) {
        return result;
    }
    
    result.header = reinterpret_cast<header_data*>(data);
    
    if (!validate_header(result.header, size)) {
        return result;
    }
    
    uint8_t* current_ptr = reinterpret_cast<uint8_t*>(data) + sizeof(header_data);
    size_t remaining_size = size - sizeof(header_data);
    
    // Parse metadata if present
    if (result.header->flags & FLAG_HAS_METADATA) {
        if (remaining_size < sizeof(header_metadata)) {
            return result; // Invalid - not enough data
        }
        result.metadata = reinterpret_cast<header_metadata*>(current_ptr);
        current_ptr += sizeof(header_metadata);
        remaining_size -= sizeof(header_metadata);
    }
    
    // Parse codec data if present
    if (result.header->flags & FLAG_HAS_CODEC_DATA) {
        if (remaining_size < sizeof(header_codec_data)) {
            return result; // Invalid - not enough data
        }
        result.codec_data = reinterpret_cast<header_codec_data*>(current_ptr);
        current_ptr += sizeof(header_codec_data);
        remaining_size -= sizeof(header_codec_data);
    }
    
    // Set payload pointer and size
    result.payload = current_ptr;
    result.payload_size = remaining_size;
    result.valid = true;
    
    return result;
}

bool BinaryProtocol::validate_header(const header_data* header, size_t total_size) {
    if (!header) return false;
    
    // Check magic number
    if (header->magic != PROTOCOL_MAGIC) {
        return false;
    }
    
    // Check version
    if (header->version != PROTOCOL_VERSION) {
        return false;
    }
    
    // Check header size matches expected size based on flags
    uint16_t expected_header_size = calculate_header_size(header->flags);
    if (header->header_size != expected_header_size) {
        return false;
    }
    
    // Check that total size is at least as large as header size
    if (total_size < header->header_size) {
        return false;
    }
    
    return true;
}

size_t BinaryProtocol::serialize(
    void* buffer,
    size_t buffer_size,
    header_data* header,
    header_metadata* metadata,
    header_codec_data* codec_data,
    const void* payload,
    size_t payload_size) {
    
    if (!header || !buffer) {
        return 0;
    }
    
    // Determine what optional data to include based on flags
    bool include_metadata = metadata != nullptr && (header->flags & FLAG_HAS_METADATA);
    bool include_codec = codec_data != nullptr && (header->flags & FLAG_HAS_CODEC_DATA);
    
    // Calculate total size
    size_t total_size = sizeof(header_data);
    if (include_metadata) total_size += sizeof(header_metadata);
    if (include_codec) total_size += sizeof(header_codec_data);
    total_size += payload_size;
    
    // Check if buffer is large enough
    if (total_size > buffer_size) {
        return 0; // Buffer too small
    }
    
    // Update header size
    header->header_size = static_cast<uint16_t>(total_size - payload_size);
    
    // Serialize into provided buffer
    uint8_t* ptr = reinterpret_cast<uint8_t*>(buffer);
    
    // Copy header
    memcpy(ptr, header, sizeof(header_data));
    ptr += sizeof(header_data);
    
    // Copy metadata if present
    if (include_metadata) {
        memcpy(ptr, metadata, sizeof(header_metadata));
        ptr += sizeof(header_metadata);
    }
    
    // Copy codec data if present
    if (include_codec) {
        memcpy(ptr, codec_data, sizeof(header_codec_data));
        ptr += sizeof(header_codec_data);
    }
    
    // Copy payload
    if (payload && payload_size > 0) {
        memcpy(ptr, payload, payload_size);
    }
    
    return total_size;
}

} // namespace Protocol
} // namespace Sesame