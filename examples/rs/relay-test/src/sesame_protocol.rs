use std::mem;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum PacketType {
    VideoFrame = 1,
    AudioFrame = 2,
    Rpc = 3,
    MuxedData = 4,
    DecoderData = 5,
}

impl From<u16> for PacketType {
    fn from(value: u16) -> Self {
        match value {
            1 => PacketType::VideoFrame,
            2 => PacketType::AudioFrame,
            3 => PacketType::Rpc,
            4 => PacketType::MuxedData,
            5 => PacketType::DecoderData,
            _ => PacketType::VideoFrame, // Default fallback
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CodecType {
    VideoVp8 = 1,
    VideoVp9 = 2,
    VideoAvc = 3,
    VideoHevc = 4,
    VideoAv1 = 5,
    AudioOpus = 64,
    AudioAac = 65,
    AudioPcm = 66,
}

impl From<u8> for CodecType {
    fn from(value: u8) -> Self {
        match value {
            1 => CodecType::VideoVp8,
            2 => CodecType::VideoVp9,
            3 => CodecType::VideoAvc,
            4 => CodecType::VideoHevc,
            5 => CodecType::VideoAv1,
            64 => CodecType::AudioOpus,
            65 => CodecType::AudioAac,
            66 => CodecType::AudioPcm,
            _ => CodecType::VideoVp8, // Default fallback
        }
    }
}

// Flag constants
pub const FLAG_HAS_CODEC_DATA: u32 = 1 << 0;
pub const FLAG_HAS_METADATA: u32 = 1 << 1;
pub const FLAG_IS_KEYFRAME: u32 = 1 << 2;

// Protocol constants
pub const PROTOCOL_MAGIC: u32 = 0x4D534553; // 'SESM'
pub const PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Clone)]
#[repr(C, packed)]
pub struct HeaderData {
    pub magic: u32,         // 0x4D534553 ('SESM') - 4 bytes (offset 0)
    pub flags: u32,         // Feature flags - 4 bytes (offset 4)
    pub pts: u64,           // Presentation timestamp - 8 bytes (offset 8) 
    pub id: u64,            // Packet identifier - 8 bytes (offset 16)
    pub version: u16,       // Protocol version (currently 1) - 2 bytes (offset 24)
    pub header_size: u16,   // Total size of all headers (excluding payload) - 2 bytes (offset 26)
    pub packet_type: u16,   // Type of packet - 2 bytes (offset 28)
    pub reserved: u16,      // Reserved - 2 bytes (offset 30)
}

#[derive(Debug, Clone)]
#[repr(C, packed)]
pub struct HeaderCodecData {
    pub sample_rate: u32,   // Audio sample rate - 4 bytes (offset 0)
    pub timebase_num: u32,  // Timebase numerator - 4 bytes (offset 4)
    pub timebase_den: u32,  // Timebase denominator - 4 bytes (offset 8)
    pub codec_profile: u16, // Codec profile - 2 bytes (offset 12)
    pub codec_level: u16,   // Codec level - 2 bytes (offset 14)
    pub width: u16,         // Frame width (video only) - 2 bytes (offset 16)
    pub height: u16,        // Frame height (video only) - 2 bytes (offset 18)
    pub codec_type: u8,     // Codec identifier - 1 byte (offset 20)
    pub channels: u8,       // Audio channels - 1 byte (offset 21)
    pub bit_depth: u8,      // Bit depth (8, 10, 12, 16) - 1 byte (offset 22)
    pub reserved: u8,       // Reserved - 1 byte (offset 23)
}

#[derive(Debug, Clone)]
#[repr(C, packed)]
pub struct HeaderMetadata {
    pub metadata: [u8; 64], // Null-terminated metadata string for routing
}

// Header size constants
pub const HEADER_DATA_SIZE: usize = mem::size_of::<HeaderData>();
pub const HEADER_CODEC_DATA_SIZE: usize = mem::size_of::<HeaderCodecData>();
pub const HEADER_METADATA_SIZE: usize = mem::size_of::<HeaderMetadata>();

#[derive(Debug)]
pub struct ParsedData<'a> {
    pub header: HeaderData,
    pub metadata: Option<HeaderMetadata>,
    pub codec_data: Option<HeaderCodecData>,
    pub payload: &'a [u8],
    pub valid: bool,
}

pub struct BinaryProtocol;

impl BinaryProtocol {
    pub fn calculate_header_size(flags: u32) -> u16 {
        let mut size = HEADER_DATA_SIZE;
        
        if flags & FLAG_HAS_METADATA != 0 {
            size += HEADER_METADATA_SIZE;
        }
        
        if flags & FLAG_HAS_CODEC_DATA != 0 {
            size += HEADER_CODEC_DATA_SIZE;
        }
        
        size as u16
    }
    
    pub fn parse_data(data: &[u8]) -> ParsedData {
        if data.len() < HEADER_DATA_SIZE {
            return ParsedData {
                header: unsafe { mem::zeroed() },
                metadata: None,
                codec_data: None,
                payload: &[],
                valid: false,
            };
        }
        
        // Parse main header
        let header = unsafe {
            std::ptr::read_unaligned(data.as_ptr() as *const HeaderData)
        };
        
        // Validate magic number
        if header.magic != PROTOCOL_MAGIC {
            return ParsedData {
                header,
                metadata: None,
                codec_data: None,
                payload: &[],
                valid: false,
            };
        }
        
        // Check if we have enough data for the complete header
        if data.len() < header.header_size as usize {
            return ParsedData {
                header,
                metadata: None,
                codec_data: None,
                payload: &[],
                valid: false,
            };
        }
        
        let mut offset = HEADER_DATA_SIZE;
        let mut metadata = None;
        let mut codec_data = None;
        
        // Parse metadata if present
        if header.flags & FLAG_HAS_METADATA != 0 {
            if offset + HEADER_METADATA_SIZE <= data.len() {
                metadata = Some(unsafe {
                    std::ptr::read_unaligned((data.as_ptr().add(offset)) as *const HeaderMetadata)
                });
                offset += HEADER_METADATA_SIZE;
            } else {
                return ParsedData {
                    header,
                    metadata: None,
                    codec_data: None,
                    payload: &[],
                    valid: false,
                };
            }
        }
        
        // Parse codec data if present
        if header.flags & FLAG_HAS_CODEC_DATA != 0 {
            if offset + HEADER_CODEC_DATA_SIZE <= data.len() {
                codec_data = Some(unsafe {
                    std::ptr::read_unaligned((data.as_ptr().add(offset)) as *const HeaderCodecData)
                });
                // Note: offset not used after this point
            } else {
                return ParsedData {
                    header,
                    metadata,
                    codec_data: None,
                    payload: &[],
                    valid: false,
                };
            }
        }
        
        // Extract payload
        let payload_start = header.header_size as usize;
        let payload = if payload_start <= data.len() {
            &data[payload_start..]
        } else {
            &[]
        };
        
        ParsedData {
            header,
            metadata,
            codec_data,
            payload,
            valid: true,
        }
    }
    
    pub fn validate_header(header: &HeaderData, total_size: usize) -> bool {
        header.magic == PROTOCOL_MAGIC &&
        header.version == PROTOCOL_VERSION &&
        (header.header_size as usize) <= total_size
    }
}

impl std::fmt::Display for PacketType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PacketType::VideoFrame => write!(f, "VIDEO"),
            PacketType::AudioFrame => write!(f, "AUDIO"),
            PacketType::Rpc => write!(f, "RPC"),
            PacketType::MuxedData => write!(f, "MUXED"),
            PacketType::DecoderData => write!(f, "DECODER"),
        }
    }
}

impl std::fmt::Display for CodecType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CodecType::VideoVp8 => write!(f, "VP8"),
            CodecType::VideoVp9 => write!(f, "VP9"),
            CodecType::VideoAvc => write!(f, "AVC"),
            CodecType::VideoHevc => write!(f, "HEVC"),
            CodecType::VideoAv1 => write!(f, "AV1"),
            CodecType::AudioOpus => write!(f, "OPUS"),
            CodecType::AudioAac => write!(f, "AAC"),
            CodecType::AudioPcm => write!(f, "PCM"),
        }
    }
}