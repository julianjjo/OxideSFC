use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Video frame data from the emulator
///
/// `data` is kept as a plain `Vec<u8>` for all Rust-side code (construction
/// via `from_raw`/`new`, pixel-level reads elsewhere in this crate), but is
/// serialized over Tauri IPC as a base64 string rather than a raw byte
/// array. `serde_json` (which Tauri's IPC layer uses) has no compact "bytes"
/// representation -- even with `serde_bytes`, `Vec<u8>` serializes as a JSON
/// array of individual numbers (e.g. `[255,0,12,...]`), which for a
/// 256x224 RGBA frame (229,376 bytes) polled up to 60 times/sec is a severe
/// size and parsing bottleneck. Base64 is ~33% larger than raw bytes but
/// roughly 3x smaller/faster than a JSON number array.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    #[serde(serialize_with = "serialize_data", deserialize_with = "deserialize_data")]
    pub data: Vec<u8>, // RGBA pixels
}

fn serialize_data<S>(data: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&STANDARD.encode(data))
}

fn deserialize_data<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
where
    D: Deserializer<'de>,
{
    let encoded = String::deserialize(deserializer)?;
    STANDARD
        .decode(encoded.as_bytes())
        .map_err(serde::de::Error::custom)
}

impl VideoFrame {
    pub fn new(width: u32, height: u32) -> Self {
        let size = (width * height * 4) as usize;
        Self {
            width,
            height,
            data: vec![0u8; size],
        }
    }

    pub fn from_raw(width: u32, height: u32, data: Vec<u8>) -> Self {
        Self {
            width,
            height,
            data,
        }
    }
}
