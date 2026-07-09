//! ROM header parsing for SNES cartridges
//! Handles LoROM, HiROM, and ExHiROM memory mappings

use serde::{Deserialize, Serialize};

/// Memory mapping types for SNES cartridges
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MemoryMapping {
    #[default]
    Unknown,
    LoRom,
    HiRom,
    ExHiRom,
}

impl MemoryMapping {
    pub fn as_str(&self) -> &'static str {
        match self {
            MemoryMapping::Unknown => "Unknown",
            MemoryMapping::LoRom => "LoROM",
            MemoryMapping::HiRom => "HiROM",
            MemoryMapping::ExHiRom => "ExHiROM",
        }
    }
}

/// ROM type classification
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum RomType {
    #[default]
    Unknown,
    Rom,
    RomRam,
    RomSram,
    RomDsram1,
    RomDsram2,
    RomDsram3,
    RomDsram4,
    RomDsram5,
    RomDsram6,
    RomDsram7,
    RomDsram8,
    RomBsram1,
    RomBsram2,
    RomBsram3,
    RomBsram4,
    RomBsram5,
    RomBsram6,
    RomBsram7,
    RomBsram8,
    RomAram1,
    RomAram2,
    RomAram3,
    RomAram4,
    RomAram5,
    RomAram6,
    RomAram7,
    RomAram8,
}

impl RomType {
    pub fn from_byte(byte: u8) -> Self {
        match byte & 0x0F {
            0x0 => RomType::Rom,
            0x1 => RomType::RomRam,
            0x2 => RomType::RomSram,
            0x3 => RomType::RomDsram1,
            0x4 => RomType::RomDsram2,
            0x5 => RomType::RomDsram3,
            0x6 => RomType::RomDsram4,
            0x7 => RomType::RomDsram5,
            0x8 => RomType::RomDsram6,
            0x9 => RomType::RomDsram7,
            0xA => RomType::RomDsram8,
            0xB => RomType::RomBsram1,
            0xC => RomType::RomBsram2,
            0xD => RomType::RomBsram3,
            0xE => RomType::RomBsram4,
            0xF => RomType::RomBsram5,
            _ => RomType::Unknown,
        }
    }
}

/// Country/region codes
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum Country {
    #[default]
    Unknown,
    Japan,
    USA,
    Europe,
    Scandinavia,
    France,
    Germany,
    Italy,
    Spain,
    Netherlands,
    Belgium,
    UnitedKingdom,
    Brazil,
    Canada,
    Australia,
    Other,
}

impl Country {
    pub fn from_byte(byte: u8) -> Self {
        match byte {
            0x00 => Country::Japan,
            0x01 => Country::USA,
            0x02 => Country::Europe,
            0x03 => Country::Scandinavia,
            0x04 => Country::France,
            0x05 => Country::Germany,
            0x06 => Country::Italy,
            0x07 => Country::Spain,
            0x08 => Country::Netherlands,
            0x09 => Country::Belgium,
            0x0A => Country::UnitedKingdom,
            0x0B => Country::Brazil,
            0x0C => Country::Canada,
            0x0D => Country::Australia,
            _ => Country::Other,
        }
    }
    
    pub fn as_str(&self) -> &'static str {
        match self {
            Country::Unknown => "Unknown",
            Country::Japan => "Japan",
            Country::USA => "USA",
            Country::Europe => "Europe",
            Country::Scandinavia => "Scandinavia",
            Country::France => "France",
            Country::Germany => "Germany",
            Country::Italy => "Italy",
            Country::Spain => "Spain",
            Country::Netherlands => "Netherlands",
            Country::Belgium => "Belgium",
            Country::UnitedKingdom => "United Kingdom",
            Country::Brazil => "Brazil",
            Country::Canada => "Canada",
            Country::Australia => "Australia",
            Country::Other => "Other",
        }
    }
}

/// ROM header information parsed from the cartridge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RomHeader {
    pub title: String,
    pub mapping: MemoryMapping,
    pub rom_type: RomType,
    pub rom_size: u32,
    pub sram_size: u32,
    pub region: Country,
    pub version: u8,
    pub destination_code: u8,
}

/// Parse the ROM header from raw data.
///
/// Delegates the actual header-location and mapper detection to
/// `oxidesfc_core::Cartridge` -- the same code the emulator itself uses to
/// map ROM bytes. This used to be a separate, independent implementation
/// that tried the HiROM offset ($FFC0) *before* the LoROM offset ($7FC0)
/// for any ROM 64KB or larger, gated only by a near-no-op validity check
/// (basically "contains at least one printable or null byte"). For a real
/// LoROM ROM like Super Mario World (512KB), that meant it would almost
/// always misread garbage mid-ROM bytes at $FFC0 as a "valid" HiROM header
/// and never even look at the real header at $7FC0. Routing through
/// `Cartridge`'s checksum-validated detection fixes that, and means the
/// library scanner and the actual emulator can no longer silently disagree
/// about what a given ROM is.
pub fn parse_rom_header(data: &[u8], _file_size: u64) -> RomHeader {
    let cart = oxidesfc_core::Cartridge::new(data.to_vec());
    let header = cart.header();

    let mapping = match header.mapper {
        oxidesfc_core::MapperType::LoRom => MemoryMapping::LoRom,
        oxidesfc_core::MapperType::HiRom => MemoryMapping::HiRom,
    };

    RomHeader {
        title: header.title.clone(),
        mapping,
        rom_type: RomType::from_byte(header.rom_type),
        rom_size: header.rom_size_bytes,
        sram_size: header.sram_size_bytes,
        region: Country::from_byte(header.region_code),
        version: header.version,
        destination_code: header.region_code,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_country_detection() {
        assert_eq!(Country::from_byte(0x00), Country::Japan);
        assert_eq!(Country::from_byte(0x01), Country::USA);
        assert_eq!(Country::from_byte(0x02), Country::Europe);
    }
}
