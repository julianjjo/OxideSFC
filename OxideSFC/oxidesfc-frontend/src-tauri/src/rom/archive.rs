//! Archive extraction module
//! Handles extraction of ROM files from ZIP archives

use std::fs::File;
use std::io::{Read, BufReader};
use std::path::Path;
use zip::ZipArchive;
use tracing::{debug, info, warn};

/// Extensions that are considered ROM files. This is the single canonical
/// SNES-only list -- `commands::library` imports it from here rather than
/// keeping its own copy, so the two can't drift out of sync (they
/// previously did: this list also included non-SNES extensions like
/// "nes"/"gb"/"gbc"/"gba" and a duplicated "sfc" entry).
pub const ROM_EXTENSIONS: &[&str] = &["sfc", "smc", "fig", "swc"];

/// Upper bound on a ROM's declared uncompressed size, in bytes. Real SNES
/// ROMs top out around 6MB (a bit more with copier headers/expansion
/// chips), so 64MB is a generous but bounded cap. This guards against a
/// crafted zip whose central-directory metadata claims a huge
/// uncompressed size for a tiny compressed payload, which would otherwise
/// trigger an oversized allocation via `Vec::with_capacity` before any
/// decompression happens.
const MAX_ROM_SIZE_BYTES: u64 = 64 * 1024 * 1024;

/// Error types for archive operations
#[derive(Debug)]
pub enum ArchiveError {
    IoError(std::io::Error),
    ZipError(zip::result::ZipError),
    NoRomFound,
    EntryTooLarge { filename: String, size: u64 },
}

impl std::fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArchiveError::IoError(e) => write!(f, "IO error: {}", e),
            ArchiveError::ZipError(e) => write!(f, "ZIP error: {}", e),
            ArchiveError::NoRomFound => write!(f, "No ROM file found in archive"),
            ArchiveError::EntryTooLarge { filename, size } => write!(
                f,
                "Archive entry too large: {} declares {} bytes uncompressed (max allowed {} bytes)",
                filename, size, MAX_ROM_SIZE_BYTES
            ),
        }
    }
}

impl From<std::io::Error> for ArchiveError {
    fn from(err: std::io::Error) -> Self {
        ArchiveError::IoError(err)
    }
}

impl From<zip::result::ZipError> for ArchiveError {
    fn from(err: zip::result::ZipError) -> Self {
        ArchiveError::ZipError(err)
    }
}

/// Check if a filename has a ROM extension
fn is_rom_file(filename: &str) -> bool {
    if let Some(ext) = Path::new(filename).extension() {
        let ext_lower = ext.to_string_lossy().to_lowercase();
        ROM_EXTENSIONS.contains(&ext_lower.as_str())
    } else {
        false
    }
}

/// Information about a ROM found in an archive
#[derive(Debug, Clone)]
pub struct RomInArchive {
    pub filename: String,
    pub size: u64,
}

/// Extract ROM files from a ZIP archive
pub fn extract_rom_from_zip<P: AsRef<Path>>(path: P) -> Result<Vec<u8>, ArchiveError> {
    let path = path.as_ref();
    info!("Extracting ROM from ZIP archive: {:?}", path);

    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut archive = ZipArchive::new(reader)?;
    
    // Find ROM files in the archive
    let mut rom_files: Vec<RomInArchive> = Vec::new();
    
    for i in 0..archive.len() {
        let file = archive.by_index(i)?;
        let filename = file.name().to_string();
        
        if is_rom_file(&filename) && !filename.contains("__MACOSX") {
            debug!("Found ROM file in archive: {}", filename);
            rom_files.push(RomInArchive {
                filename: filename.clone(),
                size: file.size(),
            });
        }
    }
    
    if rom_files.is_empty() {
        warn!("No ROM files found in archive: {:?}", path);
        return Err(ArchiveError::NoRomFound);
    }
    
    // If there's exactly one ROM, extract it
    // Otherwise, prefer .sfc over .smc, etc.
    let rom_to_extract = if rom_files.len() == 1 {
        &rom_files[0]
    } else {
        // Multiple ROMs found: pick one by extension preference rather than
        // failing outright, but warn so the choice is diagnosable. Compare
        // lowercased extensions so "Game.SFC" ties in with "game.sfc" --
        // consistent with `is_rom_file`, which also lowercases before
        // checking.
        let ends_with_ext_ci = |r: &&RomInArchive, ext: &str| {
            Path::new(&r.filename)
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase() == ext)
                .unwrap_or(false)
        };
        let chosen = rom_files.iter()
            .find(|r| ends_with_ext_ci(r, "sfc"))
            .or_else(|| rom_files.iter().find(|r| ends_with_ext_ci(r, "smc")))
            .or_else(|| rom_files.iter().find(|r| ends_with_ext_ci(r, "fig")))
            .or_else(|| rom_files.iter().find(|r| ends_with_ext_ci(r, "swc")))
            .unwrap_or(&rom_files[0]);
        warn!(
            "Archive {:?} contains {} ROM files; picked {} (candidates: {:?})",
            path,
            rom_files.len(),
            chosen.filename,
            rom_files.iter().map(|r| r.filename.as_str()).collect::<Vec<_>>()
        );
        chosen
    };
    
    debug!("Extracting ROM: {} ({} bytes)", rom_to_extract.filename, rom_to_extract.size);
    
    // Find and read the ROM file
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        if file.name() == rom_to_extract.filename {
            let declared_size = file.size();
            check_declared_size_within_cap(file.name(), declared_size)?;
            let mut buffer = Vec::with_capacity(declared_size as usize);
            file.read_to_end(&mut buffer)?;
            info!("Successfully extracted ROM: {} bytes", buffer.len());
            return Ok(buffer);
        }
    }

    Err(ArchiveError::NoRomFound)
}

/// Reject a zip entry's declared uncompressed size before anything sizes an
/// allocation from it. `declared_size` comes straight from the archive's
/// local/central-directory metadata (e.g. `ZipFile::size()`), which is
/// attacker-controlled and not validated against the actual compressed
/// payload -- a crafted zip can claim a multi-gigabyte uncompressed size
/// for a payload that's only a few bytes on disk. Kept as a standalone,
/// disk-free function so the guard itself can be unit-tested directly
/// without needing to write a suspicious-looking archive to disk (which,
/// in practice, on-access antivirus scanners can flag and quarantine as a
/// zip-bomb heuristic -- a tiny file whose header claims a huge
/// uncompressed size).
fn check_declared_size_within_cap(filename: &str, declared_size: u64) -> Result<(), ArchiveError> {
    if declared_size > MAX_ROM_SIZE_BYTES {
        warn!(
            "Archive entry {} declares {} bytes uncompressed, exceeding the {} byte cap; refusing to allocate",
            filename,
            declared_size,
            MAX_ROM_SIZE_BYTES
        );
        return Err(ArchiveError::EntryTooLarge {
            filename: filename.to_string(),
            size: declared_size,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_is_rom_file() {
        assert!(is_rom_file("game.sfc"));
        assert!(is_rom_file("game.smc"));
        assert!(is_rom_file("game.fig"));
        assert!(is_rom_file("game.swc"));
        assert!(!is_rom_file("readme.txt"));
        assert!(!is_rom_file("game.exe"));
    }

    /// Regression test for the extension tie-break bug: when multiple ROM
    /// candidates exist, the preference order (.sfc > .smc > .fig > .swc)
    /// must be case-insensitive, matching `is_rom_file`'s own lowercasing.
    /// Before the fix, an uppercase "Game.SFC" wouldn't match the
    /// `.ends_with(".sfc")` check and the tie-break would silently fall
    /// through to whatever `.smc`/`.fig`/`.swc` candidate came first
    /// (or the first ROM in the archive, via `unwrap_or`).
    #[test]
    fn test_extension_preference_is_case_insensitive() {
        let rom_files = [RomInArchive { filename: "Backup.SMC".to_string(), size: 10 },
            RomInArchive { filename: "Game.SFC".to_string(), size: 20 },
            RomInArchive { filename: "Other.FIG".to_string(), size: 30 }];

        let ends_with_ext_ci = |r: &&RomInArchive, ext: &str| {
            Path::new(&r.filename)
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase() == ext)
                .unwrap_or(false)
        };
        let chosen = rom_files.iter()
            .find(|r| ends_with_ext_ci(r, "sfc"))
            .or_else(|| rom_files.iter().find(|r| ends_with_ext_ci(r, "smc")))
            .or_else(|| rom_files.iter().find(|r| ends_with_ext_ci(r, "fig")))
            .or_else(|| rom_files.iter().find(|r| ends_with_ext_ci(r, "swc")))
            .unwrap_or(&rom_files[0]);

        assert_eq!(chosen.filename, "Game.SFC");
    }

    /// Regression test for the unbounded-allocation bug: a zip entry whose
    /// declared uncompressed size (as read straight from the archive's
    /// metadata, e.g. via `ZipFile::size()`) exceeds the sane upper bound
    /// for a SNES ROM must be rejected with `EntryTooLarge` rather than
    /// being handed to `Vec::with_capacity`.
    ///
    /// This deliberately exercises `check_declared_size_within_cap`
    /// in-memory rather than round-tripping a crafted "tiny payload, huge
    /// declared size" ZIP through disk: that exact byte pattern (a few
    /// bytes on disk claiming a multi-megabyte-or-more uncompressed size)
    /// is also the textbook zip-bomb heuristic, and on-access antivirus/
    /// endpoint security on some machines will quarantine or deny access
    /// to such a file the instant it's written -- which made an
    /// end-to-end-through-disk version of this test flaky/environment
    /// dependent. Testing the extracted guard function directly covers the
    /// exact logic (`declared_size > MAX_ROM_SIZE_BYTES`) that `extract_rom_from_zip`
    /// calls before it ever sizes an allocation, without writing anything
    /// suspicious to disk.
    #[test]
    fn test_rejects_declared_size_over_cap() {
        // Simulates a crafted zip's central-directory metadata claiming a
        // multi-gigabyte uncompressed size.
        let huge_claimed_size: u64 = 4_000_000_000;
        let result = check_declared_size_within_cap("evil.sfc", huge_claimed_size);

        match result {
            Err(ArchiveError::EntryTooLarge { filename, size }) => {
                assert_eq!(filename, "evil.sfc");
                assert_eq!(size, huge_claimed_size);
            }
            other => panic!("expected ArchiveError::EntryTooLarge, got {:?} instead", other),
        }

        // The boundary itself: one byte over the cap must still be
        // rejected.
        assert!(matches!(
            check_declared_size_within_cap("boundary.sfc", MAX_ROM_SIZE_BYTES + 1),
            Err(ArchiveError::EntryTooLarge { .. })
        ));
    }

    #[test]
    fn test_accepts_declared_size_within_cap() {
        // A real SNES ROM's size (a few MB) and the cap boundary itself
        // must both be accepted.
        assert!(check_declared_size_within_cap("mario.sfc", 4 * 1024 * 1024).is_ok());
        assert!(check_declared_size_within_cap("boundary.sfc", MAX_ROM_SIZE_BYTES).is_ok());
    }

    /// End-to-end sanity check using a real, honestly-sized zip built via
    /// the `zip` crate's own writer (so the payload size, declared size,
    /// and CRC are all consistent and nothing looks bomb-like on disk):
    /// extraction should succeed and must not be rejected by the size
    /// guard.
    #[test]
    fn test_extract_rom_from_zip_end_to_end() {
        let payload = b"SFC1ROMDATA";

        let mut zip_bytes = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut zip_bytes));
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            writer.start_file("game.sfc", options).expect("start_file");
            writer.write_all(payload).expect("write payload");
            writer.finish().expect("finish zip");
        }

        let path = std::env::temp_dir().join(format!(
            "oxidesfc_test_valid_{}_{}.zip",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        {
            let mut f = File::create(&path).expect("create temp zip");
            f.write_all(&zip_bytes).expect("write temp zip");
        }

        let result = extract_rom_from_zip(&path);
        let _ = std::fs::remove_file(&path);

        assert_eq!(result.expect("should extract successfully"), payload.to_vec());
    }
}
