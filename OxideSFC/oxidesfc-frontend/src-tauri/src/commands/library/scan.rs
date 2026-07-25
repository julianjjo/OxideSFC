//! Turning files on disk into library entries: which files count as ROMs or
//! archives, how a display title is derived from a filename, and the
//! directory walk that parses each candidate's cartridge header.

use super::Game;
use crate::rom::{extract_rom_from_zip, parse_rom_header, ROM_EXTENSIONS};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::warn;
pub(super) use walkdir::WalkDir;

const ARCHIVE_EXTENSIONS: &[&str] = &["zip", "7z", "rar"];

/// Strips common release-group/region/revision markers (parentheses,
/// brackets, region names, revision tags) from a ROM filename stem to
/// derive a cleaner display title when the cartridge header itself doesn't
/// provide one. Shared by `parse_archive_file` and `parse_rom_file`, which
/// previously each inlined an identical copy of this logic.
pub(super) fn clean_rom_title(filename: &str) -> String {
    filename
        .replace(['(', ')', '[', ']'], "")
        .replace("USA", "")
        .replace("Europe", "")
        .replace("Japan", "")
        .replace("Rev", "")
        .replace("V1", "")
        .replace("V2", "")
        .trim()
        .to_string()
}

pub(super) fn is_rom_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| ROM_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

pub(super) fn is_archive_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| ARCHIVE_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Normalizes a stored/scanned file path for equality comparison so that
/// duplicate detection isn't fooled by superficially different but
/// equivalent paths -- e.g. differently-cased drive letters or directory
/// names (Windows paths are case-insensitive), a trailing separator, or `.`
/// path segments introduced by how a folder was picked/re-scanned.
///
/// `Path::canonicalize` resolves all of that (and symlinks) by consulting
/// the filesystem, but it fails outright if the path no longer exists (e.g.
/// the file was deleted or the drive was unmounted between scans). In that
/// case we fall back to a lowercased raw-string comparison rather than
/// erroring out of the whole add_game_folder call -- a best-effort
/// duplicate check is better than none, and this only affects comparison,
/// never what gets stored.
pub(super) fn normalize_path_for_comparison(path: &str) -> String {
    match std::path::Path::new(path).canonicalize() {
        Ok(canonical) => canonical.to_string_lossy().to_lowercase(),
        Err(_) => path.to_lowercase(),
    }
}

/// Parse a ROM file from an archive (ZIP)
pub(super) fn parse_archive_file(path: &PathBuf) -> Option<Game> {
    // For archives, we need to extract the ROM first
    let rom_data = match extract_rom_from_zip(path) {
        Ok(data) => data,
        Err(e) => {
            warn!("Failed to extract ROM from archive {:?}: {}", path, e);
            return None;
        }
    };
    
    let file_name = path.file_name()?.to_str()?.to_string();
    let file_size = rom_data.len() as u64;
    
    // Parse the extracted ROM header
    let header = parse_rom_header(&rom_data, file_size);
    
    let title = if !header.title.is_empty() {
        header.title.clone()
    } else {
        path.file_stem()
            .and_then(|s| s.to_str())
            .map(clean_rom_title)
            .unwrap_or_else(|| file_name.clone())
    };

    Some(Game {
        id: uuid::Uuid::new_v4().to_string(),
        title,
        file_path: path.to_str()?.to_string(),
        file_name,
        file_size,
        rom_type: header.mapping.as_str().to_string(),
        sram_size: header.sram_size,
        country: header.region.as_str().to_string(),
        play_count: 0,
        last_played: None,
        favorite: false,
        custom_cover_path: None,
        created_at: chrono::Utc::now().to_rfc3339(),
        updated_at: chrono::Utc::now().to_rfc3339(),
        total_play_seconds: 0,
    })
}

pub(super) fn parse_rom_file(path: &PathBuf) -> Option<Game> {
    let metadata = fs::metadata(path).ok()?;
    let file_name = path.file_name()?.to_str()?.to_string();
    let file_size = metadata.len();

    // Read the whole ROM. Copier-header detection depends on the *true*
    // file length mod 32KB (see rom::header::parse_rom_header) -- a partial
    // read would make a real headered ROM look unheadered (or vice versa)
    // whenever the truncation point didn't land on a 32KB boundary, silently
    // shifting every header field. SNES ROMs top out around 6MB, so reading
    // the full file is cheap.
    let buffer = fs::read(path).ok()?;

    // Parse ROM header
    let header = parse_rom_header(&buffer, file_size);
    
    // Use header title if available, otherwise fall back to filename
    let title = if !header.title.is_empty() {
        header.title.clone()
    } else {
        path.file_stem()
            .and_then(|s| s.to_str())
            .map(clean_rom_title)
            .unwrap_or_else(|| file_name.clone())
    };

    Some(Game {
        id: uuid::Uuid::new_v4().to_string(),
        title,
        file_path: path.to_str()?.to_string(),
        file_name,
        file_size,
        rom_type: header.mapping.as_str().to_string(),
        sram_size: header.sram_size,
        country: header.region.as_str().to_string(),
        play_count: 0,
        last_played: None,
        favorite: false,
        custom_cover_path: None,
        created_at: chrono::Utc::now().to_rfc3339(),
        updated_at: chrono::Utc::now().to_rfc3339(),
        total_play_seconds: 0,
    })
}
