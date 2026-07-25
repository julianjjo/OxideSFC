use super::*;
use crate::bus::MemoryBus;

fn header_stub() -> CartridgeHeader {
    CartridgeHeader {
        title: String::new(),
        mapper: MapperType::LoRom,
        rom_type: 0,
        rom_size_code: 0,
        rom_size_bytes: 0,
        sram_size_code: 0,
        sram_size_bytes: 0,
        region_code: 0,
        version: 0,
        checksum: 0,
        checksum_complement: 0,
        checksum_complement_valid: false,
        computed_checksum: 0,
        computed_checksum_matches: false,
        had_copier_header: false,
    }
}

/// Builds a synthetic LoROM image with a valid, internally-consistent
/// header (checksum ^ complement == 0xFFFF) at $7FC0, optionally
/// prefixed with a 512-byte copier header.
fn make_valid_lorom(size: usize, with_copier_header: bool) -> Vec<u8> {
    let mut rom = vec![0x42u8; size];
    let header_offset = 0x7FC0;
    rom[header_offset..header_offset + 21].copy_from_slice(b"TEST ROM             ");
    rom[header_offset + 0x15] = 0x20; // LoROM, slow
    rom[header_offset + 0x17] = 0x09; // rom size code
    rom[header_offset + 0x18] = 0x00; // no sram
    rom[header_offset + 0x19] = 0x01; // USA

    // Real cartridges are mastered so that summing *every byte* of the
    // ROM (a plain byte sum, confirmed against the actual SMW ROM's
    // checksum field) equals the stored checksum, including the
    // checksum/complement bytes themselves. Since complement is the
    // bitwise-NOT of checksum, each (checksum_byte, complement_byte)
    // pair always sums to exactly 255 -- so these 4 header bytes
    // contribute a constant 510 to the total regardless of the
    // checksum's actual value. Zero the fields, sum everything else,
    // then add that constant back in.
    rom[header_offset + 0x1C..header_offset + 0x20].fill(0);
    let sum_without_checksum_fields = Cartridge::compute_checksum(&rom);
    let checksum = sum_without_checksum_fields.wrapping_add(510);
    let complement = !checksum;
    rom[header_offset + 0x1C..header_offset + 0x1E].copy_from_slice(&complement.to_le_bytes());
    rom[header_offset + 0x1E..header_offset + 0x20].copy_from_slice(&checksum.to_le_bytes());

    if with_copier_header {
        let mut with_header = vec![0xAAu8; 512];
        with_header.extend_from_slice(&rom);
        with_header
    } else {
        rom
    }
}

#[test]
fn lorom_mapping() {
    // Minimal mock generic LoROM length
    let cart = Cartridge {
        rom: vec![0x42; 0x100000], // 1 MB
        sram: vec![],
        mapper: MapperType::LoRom,
        has_sram: false,
        header: header_stub(),
    };

    // Address 0x008000 in LoROM maps to 0x000000 physical
    assert_eq!(cart.map_lorom(0x008000), Some(0x000000));

    // Address 0x018000 in LoROM maps to 0x008000 physical
    assert_eq!(cart.map_lorom(0x018000), Some(0x008000));
}

#[test]
fn hirom_mapping() {
    let cart = Cartridge {
        rom: vec![0x42; 0x100000], // 1 MB
        sram: vec![],
        mapper: MapperType::HiRom,
        has_sram: false,
        header: header_stub(),
    };

    // Address 0xC00000 in HiROM maps to 0x000000 physical
    assert_eq!(cart.map_hirom(0xC00000), Some(0x000000));

    // Address 0x008000 in HiROM maps to 0x008000 physical
    assert_eq!(cart.map_hirom(0x008000), Some(0x008000));
}

#[test]
fn hirom_banks_40_to_7d_mirror_the_full_rom_image() {
    // Real HiROM maps banks $40-$7D as full-64KB ROM banks, the SlowROM
    // image of $C0-$FD. These were previously unmapped entirely, so
    // every read there returned stale open-bus bytes -- DKC (a HiROM
    // cart) reads data through these banks, which surfaced as garbage
    // graphics. A 4MB image needs the full $40-$7D range to be
    // reachable (bank & 0x3F * 64KB spans all 4MB).
    let cart = Cartridge {
        rom: (0..0x400000u32).map(|i| (i >> 16) as u8).collect(), // 4MB, each byte = its bank index
        sram: vec![],
        mapper: MapperType::HiRom,
        has_sram: false,
        header: header_stub(),
    };

    // Bank $40 offset $0000 maps to physical 0x000000 (same as $C0).
    assert_eq!(cart.map_hirom(0x400000), Some(0x000000));
    assert_eq!(cart.map_hirom(0x400000), cart.map_hirom(0xC00000));

    // Low half of the bank is ROM too (unlike the $00-$3F system banks).
    assert_eq!(cart.map_hirom(0x412345), Some(0x012345));

    // Top of the window: $7D:FFFF -> physical 0x3DFFFF.
    assert_eq!(cart.map_hirom(0x7DFFFF), Some(0x3DFFFF));

    // Banks $7E/$7F are WRAM, never cartridge -- the mapper itself
    // must not claim them (the bus routes them away first, but the
    // mapper shouldn't lie about them either).
    assert_eq!(cart.map_hirom(0x7E0000), None);
}

#[test]
fn hirom_sram_window_maps_banks_20_to_3f_at_6000_to_7fff() {
    let mut cart = Cartridge {
        rom: vec![0x42; 0x100000],
        sram: vec![0; 0x2000], // 8KB chip
        mapper: MapperType::HiRom,
        has_sram: true,
        header: header_stub(),
    };

    // Write through the primary window and read back through the
    // $A0-$BF fast mirror.
    cart.write_u8(0x206000, 0xAB).unwrap();
    assert_eq!(cart.read_u8(0x206000).unwrap(), 0xAB);
    assert_eq!(cart.read_u8(0xA06000).unwrap(), 0xAB, "$A0-$BF must mirror the $20-$3F window");

    // An 8KB chip mirrors across every bank of the window (partial
    // address decoding): bank $21's slice wraps back onto bank $20's.
    assert_eq!(cart.read_u8(0x216000).unwrap(), 0xAB);

    // ROM reads outside $6000-$7FFF are untouched ($00:8000 maps to
    // physical 0x8000, well within the 1MB test image).
    assert_eq!(cart.read_u8(0x008000).unwrap(), 0x42);

    // Offsets below $6000 in these banks must not hit SRAM.
    assert!(cart.write_u8(0x205FFF, 0x11).is_err());
}

#[test]
fn strips_512_byte_copier_header() {
    let rom = make_valid_lorom(0x80000, true);
    assert_eq!(rom.len(), 0x80000 + 512);

    let cart = Cartridge::new(rom);
    assert!(cart.header().had_copier_header);
    assert_eq!(cart.rom_len(), 0x80000);
    assert_eq!(cart.header().title, "TEST ROM");
    assert_eq!(cart.mapper, MapperType::LoRom);
}

#[test]
fn no_copier_header_when_size_is_exact_multiple_of_32kb() {
    let rom = make_valid_lorom(0x80000, false);
    assert_eq!(rom.len() % 0x8000, 0);

    let cart = Cartridge::new(rom);
    assert!(!cart.header().had_copier_header);
    assert_eq!(cart.rom_len(), 0x80000);
}

#[test]
fn checksum_complement_validates_correct_header() {
    let rom = make_valid_lorom(0x80000, false);
    let cart = Cartridge::new(rom);

    assert!(cart.header().checksum_complement_valid);
    assert!(cart.header().computed_checksum_matches);
}

#[test]
fn corrupted_checksum_is_detected() {
    let mut rom = make_valid_lorom(0x80000, false);
    // Flip a byte in the ROM body (not the header) so the stored checksum
    // no longer matches the actual contents -- this must be detectable.
    rom[0x1000] ^= 0xFF;

    let cart = Cartridge::new(rom);
    assert!(cart.header().checksum_complement_valid, "header is still self-consistent");
    assert!(
        !cart.header().computed_checksum_matches,
        "recomputed checksum must catch corrupted ROM data even when the header itself is well-formed"
    );
}

#[test]
fn garbage_rom_size_code_is_clamped_not_overflowed() {
    // A non-SNES or corrupt file can still produce a header-shaped
    // region with an out-of-range size byte. Found by independent
    // review: this used to overflow to u32::MAX (a nonsense
    // multi-gigabyte "ROM size") instead of being clamped like the
    // SRAM path already was.
    let mut rom = make_valid_lorom(0x80000, false);
    rom[0x7FC0 + 0x17] = 0xFF; // garbage rom_size_code
    // Recompute the checksum so this still parses as a self-consistent
    // header (the size byte is outside the checksummed fields' range).
    rom[0x7FC0 + 0x1C..0x7FC0 + 0x20].fill(0);
    let sum = Cartridge::compute_checksum(&rom);
    let checksum = sum.wrapping_add(510);
    rom[0x7FC0 + 0x1C..0x7FC0 + 0x1E].copy_from_slice(&(!checksum).to_le_bytes());
    rom[0x7FC0 + 0x1E..0x7FC0 + 0x20].copy_from_slice(&checksum.to_le_bytes());

    let cart = Cartridge::new(rom);
    assert_eq!(cart.header().rom_size_code, 0xFF);
    assert_eq!(cart.header().rom_size_bytes, MAX_DECLARED_ROM_BYTES, "must clamp instead of overflowing to a nonsense multi-gigabyte value");
}

#[test]
fn without_stripping_header_offset_silently_shifts_and_breaks_checksum() {
    // Regression guard: if the copier header were NOT stripped, the
    // header bytes read at 0x7FC0 would be 512 bytes into what's
    // actually still copier-header + early ROM data, so the checksum
    // must fail to validate. This pins the exact failure mode of the
    // original bug.
    let rom_with_header = make_valid_lorom(0x80000, true);
    let unstripped_view = &rom_with_header[..]; // pretend no stripping happened
    let header = Cartridge::parse_candidate(unstripped_view, 0x7FC0, MapperType::LoRom, false)
        .expect("candidate should at least parse bytes");
    assert!(
        !header.checksum_complement_valid,
        "reading the header at the un-shifted offset must not look valid"
    );
}
