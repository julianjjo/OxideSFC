use crate::bus::{BusResult, MemoryBus};
use crate::error::EmulationError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapperType {
    LoRom,
    HiRom,
}

/// Parsed SNES cartridge header (the 32-byte "ROM info area" at the end of a bank),
/// plus the validation signals needed to prove a ROM was loaded and mapped correctly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CartridgeHeader {
    pub title: String,
    pub mapper: MapperType,
    pub rom_type: u8,
    pub rom_size_code: u8,
    pub rom_size_bytes: u32,
    pub sram_size_code: u8,
    pub sram_size_bytes: u32,
    pub region_code: u8,
    pub version: u8,
    /// Checksum field stored in the header ($xFDE-$xFDF).
    pub checksum: u16,
    /// One's complement of `checksum`, also stored in the header ($xFDC-$xFDD).
    pub checksum_complement: u16,
    /// True if `checksum ^ checksum_complement == 0xFFFF`, i.e. the header is
    /// internally consistent (a strong signal this is a real SNES header and
    /// not a coincidental byte pattern).
    pub checksum_complement_valid: bool,
    /// Checksum actually computed over the (post-copier-header-strip) ROM bytes,
    /// using the standard SNES convention of mirroring any partial tail to fill
    /// out to the next power of two.
    pub computed_checksum: u16,
    /// True if `computed_checksum == checksum`. This is the strongest available
    /// proof that the ROM bytes were read from disk and mapped correctly: it
    /// recomputes Nintendo's own checksum over the entire ROM image and checks
    /// it byte-for-byte against the value baked into the cartridge.
    pub computed_checksum_matches: bool,
    /// True if a 512-byte copier header was detected and stripped before
    /// parsing (common for .smc dumps).
    pub had_copier_header: bool,
}

pub struct Cartridge {
    rom: Vec<u8>,
    sram: Vec<u8>,
    mapper: MapperType,
    has_sram: bool,
    header: CartridgeHeader,
}

/// Maximum plausible original-hardware SRAM size. Used to clamp the size
/// decoded from a (possibly garbage) header byte so a corrupt/non-SNES ROM
/// can't trigger a huge allocation.
const MAX_SRAM_BYTES: u32 = 0x20000; // 128KB

/// Maximum plausible commercial SNES ROM size (the largest known carts are
/// ~6MB; this leaves generous headroom). Used to clamp the *declared* size
/// read from a (possibly garbage) header byte -- this is purely a display
/// value (actual memory mapping always uses the real `rom.len()`), but an
/// unclamped `1024 << code` for a garbage code byte could otherwise report
/// a nonsense multi-gigabyte size.
const MAX_DECLARED_ROM_BYTES: u32 = 0x400_0000; // 64MB

impl Cartridge {
    pub fn new(rom: Vec<u8>) -> Self {
        let (rom, had_copier_header) = Self::strip_copier_header(rom);
        let (mapper, header) = Self::detect_header(&rom, had_copier_header);

        let sram_len = header.sram_size_bytes as usize;
        let has_sram = sram_len > 0;
        let sram = vec![0; sram_len];

        Self {
            rom,
            sram,
            mapper,
            has_sram,
            header,
        }
    }

    /// Strips a leading 512-byte copier header if one is present.
    ///
    /// Real SNES ROM images are always a multiple of 32KB (0x8000): the
    /// cartridge's ROM chips come in power-of-two sizes. Common copier
    /// formats (e.g. plain .smc dumps) prepend exactly 512 bytes before the
    /// real image, so a leftover remainder of exactly 512 after dividing by
    /// 0x8000 is the standard signal that this extra header is present and
    /// must be removed before any header offset or memory-mapping math runs.
    /// Without this, every ROM-mapped read is silently shifted by 512 bytes.
    fn strip_copier_header(mut rom: Vec<u8>) -> (Vec<u8>, bool) {
        if rom.len() > 512 && rom.len() % 0x8000 == 512 {
            rom.drain(0..512);
            (rom, true)
        } else {
            (rom, false)
        }
    }

    /// Computes the SNES-style checksum over `rom`: sum of all bytes mod
    /// 0x10000, mirroring any non-power-of-two tail to fill out to the next
    /// power of two (the convention Nintendo's own header checksum follows).
    fn compute_checksum(rom: &[u8]) -> u16 {
        if rom.is_empty() {
            return 0;
        }

        let mut pow2 = 1usize;
        while pow2 * 2 <= rom.len() {
            pow2 *= 2;
        }

        let mut sum: u32 = rom[..pow2].iter().map(|&b| b as u32).sum();

        if rom.len() > pow2 {
            let remainder = &rom[pow2..];
            let mut covered = 0usize;
            while covered < pow2 {
                let take = remainder.len().min(pow2 - covered);
                sum += remainder[..take].iter().map(|&b| b as u32).sum::<u32>();
                covered += take;
            }
        }

        (sum & 0xFFFF) as u16
    }

    fn header_size_bytes(code: u8) -> u32 {
        if code == 0 {
            0
        } else {
            1024u32
                .checked_shl(code as u32)
                .unwrap_or(u32::MAX)
                .min(MAX_SRAM_BYTES)
        }
    }

    fn parse_title(bytes: &[u8]) -> String {
        String::from_utf8_lossy(bytes)
            .trim_end_matches(['\0', ' '])
            .to_string()
    }

    /// Builds a `CartridgeHeader` from the 32-byte header region starting at
    /// `offset` within `rom`, without yet knowing if this is the right
    /// candidate (LoROM vs HiROM) -- that decision is made by the caller
    /// using `checksum_complement_valid` from both candidates.
    fn parse_candidate(
        rom: &[u8],
        offset: usize,
        mapper: MapperType,
        had_copier_header: bool,
    ) -> Option<CartridgeHeader> {
        if rom.len() < offset + 0x20 {
            return None;
        }
        let h = &rom[offset..offset + 0x20];

        let title = Self::parse_title(&h[0..21]);
        // Offset 0x15 is the *map mode* byte (used only for mapper
        // detection in `detect_header`), not the cartridge type -- that's
        // 0x16. Mixing these up was a real bug in earlier versions of this
        // parser (and still is in the frontend's duplicate parser).
        let rom_type = h[0x16];
        let rom_size_code = h[0x17];
        let sram_size_code = h[0x18];
        let region_code = h[0x19];
        let version = h[0x1B];
        let checksum_complement = u16::from_le_bytes([h[0x1C], h[0x1D]]);
        let checksum = u16::from_le_bytes([h[0x1E], h[0x1F]]);
        let checksum_complement_valid = checksum ^ checksum_complement == 0xFFFF;
        let computed_checksum = Self::compute_checksum(rom);

        Some(CartridgeHeader {
            title,
            mapper,
            rom_type,
            rom_size_code,
            rom_size_bytes: if rom_size_code == 0 {
                0
            } else {
                1024u32
                    .checked_shl(rom_size_code as u32)
                    .unwrap_or(u32::MAX)
                    .min(MAX_DECLARED_ROM_BYTES)
            },
            sram_size_code,
            sram_size_bytes: Self::header_size_bytes(sram_size_code),
            region_code,
            version,
            checksum,
            checksum_complement,
            checksum_complement_valid,
            computed_checksum,
            computed_checksum_matches: computed_checksum == checksum,
            had_copier_header,
        })
    }

    /// Detects whether `rom` (already stripped of any copier header) is
    /// LoROM or HiROM and parses its header.
    ///
    /// Preference order:
    /// 1. Whichever candidate location has an internally-consistent checksum
    ///    (`checksum ^ complement == 0xFFFF`) -- this is far more reliable
    ///    than guessing from a single mode byte, since real header bytes at
    ///    the wrong offset can coincidentally look plausible.
    /// 2. If both or neither validate, fall back to the cartridge map-mode
    ///    byte heuristic (bit 0 of the byte at header offset +0x15).
    fn detect_header(rom: &[u8], had_copier_header: bool) -> (MapperType, CartridgeHeader) {
        let lo = Self::parse_candidate(rom, 0x7FC0, MapperType::LoRom, had_copier_header);
        let hi = Self::parse_candidate(rom, 0xFFC0, MapperType::HiRom, had_copier_header);

        let lo_valid = lo.as_ref().is_some_and(|h| h.checksum_complement_valid);
        let hi_valid = hi.as_ref().is_some_and(|h| h.checksum_complement_valid);

        let chosen = if lo_valid && !hi_valid {
            lo
        } else if hi_valid && !lo_valid {
            hi
        } else {
            // Neither (or both) checksums validate on their own -- fall back
            // to the map-mode byte heuristic (bit 0 of the byte at header
            // offset +0x15). HiROM is checked first: on a true HiROM
            // cartridge the byte the LoROM check inspects isn't a real
            // header field, so it must not be allowed to override a
            // positive HiROM detection.
            let hi_is_hirom_mode = rom.get(0xFFC0 + 0x15).is_some_and(|b| b & 0x01 != 0);
            if hi_is_hirom_mode {
                hi.or(lo)
            } else {
                lo.or(hi)
            }
        };

        match chosen {
            Some(header) => (header.mapper, header),
            None => (
                MapperType::LoRom,
                CartridgeHeader {
                    title: String::new(),
                    mapper: MapperType::LoRom,
                    rom_type: 0,
                    rom_size_code: 0,
                    rom_size_bytes: rom.len() as u32,
                    sram_size_code: 0,
                    sram_size_bytes: 0,
                    region_code: 0,
                    version: 0,
                    checksum: 0,
                    checksum_complement: 0,
                    checksum_complement_valid: false,
                    computed_checksum: Self::compute_checksum(rom),
                    computed_checksum_matches: false,
                    had_copier_header,
                },
            ),
        }
    }

    /// The parsed, validated cartridge header.
    pub fn header(&self) -> &CartridgeHeader {
        &self.header
    }

    /// Length of the (copier-header-stripped) ROM image actually mapped.
    pub fn rom_len(&self) -> usize {
        self.rom.len()
    }

    /// The battery-backed SRAM contents, for save states / .srm files.
    pub fn sram(&self) -> &[u8] {
        &self.sram
    }

    /// Mutable SRAM contents, for save states / .srm files.
    pub fn sram_mut(&mut self) -> &mut [u8] {
        &mut self.sram
    }

    fn map_lorom(&self, addr: u32) -> Option<usize> {
        let bank = (addr >> 16) as u8;
        let offset = (addr & 0xFFFF) as u16;

        if (bank <= 0x3F || (0x80..=0xBF).contains(&bank)) && offset >= 0x8000 {
            let rom_addr = ((bank & 0x7F) as usize) * 0x8000 + ((offset & 0x7FFF) as usize);
            if rom_addr < self.rom.len() {
                return Some(rom_addr);
            }
        }

        if (0x40..=0x7D).contains(&bank) || (0xC0..=0xFF).contains(&bank) {
            let rom_addr = ((bank & 0x7F) as usize) * 0x8000 + ((offset & 0x7FFF) as usize);
            if rom_addr < self.rom.len() {
                return Some(rom_addr);
            }
        }

        None
    }

    /// LoROM SRAM window: banks $70-$7D (and their $F0-$FD fast mirrors),
    /// offsets $0000-$7FFF. Each bank contributes a 32KB slice; smaller
    /// SRAM chips (SMW has 2KB) mirror across the window, matching real
    /// hardware's partial address decoding. Returns an index into `sram`.
    fn map_lorom_sram(&self, addr: u32) -> Option<usize> {
        if !self.has_sram || self.sram.is_empty() {
            return None;
        }
        let bank = (addr >> 16) as u8;
        let offset = (addr & 0xFFFF) as u16;
        let in_sram_banks = (0x70..=0x7D).contains(&bank) || (0xF0..=0xFD).contains(&bank);
        if in_sram_banks && offset < 0x8000 {
            let idx = ((bank & 0x0F) as usize) * 0x8000 + (offset as usize);
            return Some(idx % self.sram.len());
        }
        None
    }

    /// HiROM SRAM window: banks $20-$3F (and their $A0-$BF fast mirrors),
    /// offsets $6000-$7FFF. Each bank contributes an 8KB slice; smaller
    /// SRAM chips mirror across the window via the modulo, matching real
    /// hardware's partial address decoding (same idiom as
    /// `map_lorom_sram`). Returns an index into `sram`.
    fn map_hirom_sram(&self, addr: u32) -> Option<usize> {
        if !self.has_sram || self.sram.is_empty() {
            return None;
        }
        let bank = (addr >> 16) as u8;
        let offset = (addr & 0xFFFF) as u16;
        let in_sram_banks = (0x20..=0x3F).contains(&bank) || (0xA0..=0xBF).contains(&bank);
        if in_sram_banks && (0x6000..0x8000).contains(&offset) {
            let idx = ((bank & 0x1F) as usize) * 0x2000 + ((offset - 0x6000) as usize);
            return Some(idx % self.sram.len());
        }
        None
    }

    fn map_hirom(&self, addr: u32) -> Option<usize> {
        let bank = (addr >> 16) as u8;
        let offset = (addr & 0xFFFF) as u16;

        // Banks $C0-$FF and $40-$7D both expose the full ROM image 64KB
        // per bank ($40-$7D is the SlowROM image of $C0-$FD). Leaving
        // $40-$7D unmapped made every read there return stale open-bus
        // bytes -- DKC sources DMA tile uploads from these banks, which
        // rendered as garbage tile rows over the Rare/Nintendo intro
        // screens.
        if (0xC0..=0xFF).contains(&bank) || (0x40..=0x7D).contains(&bank) {
            let rom_addr = ((bank & 0x3F) as usize) * 0x10000 + (offset as usize);
            if rom_addr < self.rom.len() {
                return Some(rom_addr);
            }
        }

        if (bank <= 0x3F || (0x80..=0xBF).contains(&bank)) && offset >= 0x8000 {
            let rom_addr = ((bank & 0x3F) as usize) * 0x10000 + (offset as usize);
            if rom_addr < self.rom.len() {
                return Some(rom_addr);
            }
        }

        None
    }
}

impl MemoryBus for Cartridge {
    fn read_u8(&mut self, addr: u32) -> BusResult<u8> {
        // SRAM takes priority over the ROM mirror in its window -- SMW's
        // save-file reads (`LDA.L SaveData,X` at $700000+) must see what
        // the save routine wrote, not ROM bytes.
        let sram_idx = match self.mapper {
            MapperType::LoRom => self.map_lorom_sram(addr),
            MapperType::HiRom => self.map_hirom_sram(addr),
        };
        if let Some(idx) = sram_idx {
            return Ok(self.sram[idx]);
        }

        let rom_addr = match self.mapper {
            MapperType::LoRom => self.map_lorom(addr),
            MapperType::HiRom => self.map_hirom(addr),
        };

        if let Some(r_addr) = rom_addr {
            return Ok(self.rom[r_addr]);
        }

        Err(EmulationError::OpenBus)
    }

    fn write_u8(&mut self, addr: u32, value: u8) -> BusResult<()> {
        // Only SRAM is writable; ROM/unmapped writes report open-bus so
        // the bus can ignore them.
        let sram_idx = match self.mapper {
            MapperType::LoRom => self.map_lorom_sram(addr),
            MapperType::HiRom => self.map_hirom_sram(addr),
        };
        if let Some(idx) = sram_idx {
            self.sram[idx] = value;
            return Ok(());
        }
        Err(EmulationError::OpenBus)
    }
}

#[cfg(test)]
mod tests;
