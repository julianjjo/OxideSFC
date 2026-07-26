//! The game library: scanning folders for ROMs, persisting the resulting
//! entries to `library.json`, and the Tauri commands the frontend calls to
//! query and mutate them.
//!
//! `scan` does the filesystem-to-`Game` work and `store` owns the JSON file;
//! this module holds the `Game` shape itself and the command handlers.

mod scan;
mod store;

#[cfg(test)]
mod tests;

pub use store::{
    add_play_seconds_to_file, get_game_by_id, record_play_start, set_cover_file,
    set_cover_file_for_all,
};

use crate::AppState;
use scan::{is_archive_file, is_rom_file, normalize_path_for_comparison, parse_archive_file, parse_rom_file, WalkDir};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use store::{get_games_for_mutation, get_library_path, library_guard, save_games};
use tauri::State;
use tracing::{debug, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Game {
    pub id: String,
    pub title: String,
    pub file_path: String,
    pub file_name: String,
    pub file_size: u64,
    pub rom_type: String,
    pub sram_size: u32,
    pub country: String,
    pub play_count: u32,
    pub last_played: Option<String>,
    pub favorite: bool,
    pub custom_cover_path: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// Accumulated real-world seconds of emulation time for this game.
    /// `#[serde(default)]` so `library.json` files written before this field
    /// existed still deserialize (they simply get 0).
    #[serde(default)]
    pub total_play_seconds: u64,
    /// File name (not a full path) of this game's cover inside the app's covers
    /// directory -- see `commands::covers`.
    ///
    /// Deliberately a bare file name so the library survives the data directory
    /// moving or being restored on another machine; the frontend joins it with
    /// the directory reported by `get_covers_dir`. This is distinct from
    /// `custom_cover_path`, which is reserved for an image the user points at
    /// directly, wherever it happens to live.
    #[serde(default)]
    pub cover_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    pub games: Vec<Game>,
    pub total: usize,
    pub errors: Vec<String>,
}

#[tauri::command]
pub fn get_games() -> Result<Vec<Game>, String> {
    // Load games from the library database
    let library_path = get_library_path()?;

    if !library_path.exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(&library_path)
        .map_err(|e| format!("Failed to read library: {}", e))?;

    match serde_json::from_str(&content) {
        Ok(games) => Ok(games),
        Err(e) => {
            // The file exists but isn't valid JSON (e.g. truncated by a
            // crash mid-write). Recover the same way a missing file is
            // handled rather than surfacing a hard error to the UI.
            warn!("Failed to parse library file, falling back to an empty library: {}", e);
            Ok(Vec::new())
        }
    }
}

/// Scan a folder and persist whatever ROMs it holds into `library.json`.
///
/// `recursive` is `Option` rather than a plain `bool` so the parameter could be
/// introduced without breaking callers that already invoke this with just a
/// path; it defaults to descending into subfolders, which is what this command
/// hardcoded before. The library settings screen now forwards the user's
/// `scan_recursive` preference here -- that setting existed and was persisted,
/// but nothing had ever read it, so turning it off changed nothing.
#[tauri::command]
pub fn add_game_folder(path: String, recursive: Option<bool>) -> Result<ScanResult, String> {
    let recursive = recursive.unwrap_or(true);
    info!("Adding game folder: {} (recursive: {})", path, recursive);

    // Scan the directory
    let result = scan_directory(path.clone(), recursive)?;

    // Hold the library lock for the entire get-modify-save sequence so a
    // concurrent add_game_folder/remove_game call can't interleave with
    // this one and lose an update.
    let _library_guard = library_guard();

    // Load existing games (propagate errors instead of silently treating a
    // corrupt/unreadable library.json as an empty library, which would
    // otherwise cause the save below to permanently wipe it)
    let mut games = get_games_for_mutation()?;

    // Add new games (avoiding duplicates by file_path). Compare
    // canonicalized paths rather than raw strings so re-adding the same
    // folder via a differently-cased path (Windows paths are
    // case-insensitive) or a differently-formed-but-equivalent path (e.g.
    // trailing slash, `.` segments) is correctly recognized as a duplicate
    // instead of duplicating every game in the folder.
    let existing_paths: std::collections::HashSet<_> = games.iter()
        .map(|g| normalize_path_for_comparison(&g.file_path))
        .collect();

    for game in result.games {
        if !existing_paths.contains(&normalize_path_for_comparison(&game.file_path)) {
            games.push(game);
        }
    }

    // Save updated library
    save_games(&games)?;

    let total = games.len();
    info!("Added games to library. Total: {}", total);

    Ok(ScanResult {
        games,
        total,
        errors: result.errors,
    })
}

#[tauri::command]
pub fn remove_game(game_id: String) -> Result<(), String> {
    info!("Removing game: {}", game_id);

    let _library_guard = library_guard();

    let mut games = get_games_for_mutation()?;
    games.retain(|g| g.id != game_id);
    save_games(&games)?;

    Ok(())
}

/// Flips the `favorite` flag of the game matching `game_id` within `games`
/// in place, returning the new value. Split out from the `#[tauri::command]`
/// wrapper below so it's unit-testable against an in-memory `Vec<Game>`
/// without touching the real `library.json` on disk.
fn toggle_favorite_in(games: &mut [Game], game_id: &str) -> Result<bool, String> {
    let game = games
        .iter_mut()
        .find(|g| g.id == game_id)
        .ok_or_else(|| format!("Game not found: {}", game_id))?;
    game.favorite = !game.favorite;
    Ok(game.favorite)
}

/// Flips a game's `favorite` flag and persists the change. Returns the new
/// value so the caller doesn't need a separate round-trip to learn it.
#[tauri::command]
pub fn toggle_game_favorite(game_id: String) -> Result<bool, String> {
    info!("Toggling favorite for game: {}", game_id);

    let _library_guard = library_guard();

    let mut games = get_games_for_mutation()?;
    let new_value = toggle_favorite_in(&mut games, &game_id)?;
    save_games(&games)?;

    Ok(new_value)
}

/// Wipes every entry from the library, without touching the underlying ROM
/// files on disk -- this only clears `library.json`.
#[tauri::command]
pub fn clear_library() -> Result<(), String> {
    info!("Clearing library");

    let _library_guard = library_guard();

    save_games(&[])?;

    Ok(())
}

/// Re-scans every folder currently configured in `settings.json`'s
/// `library.folders` (the authoritative list of ROM folders -- `library.json`
/// itself stores only games, never folder paths) and merges any newly found
/// games into the existing library, deduping by normalized file path exactly
/// like `add_game_folder` does. Existing games not found again are left
/// untouched (removal of missing files is `verify_library`'s job, not this
/// one's).
#[tauri::command]
pub fn rescan_library(state: State<AppState>) -> Result<ScanResult, String> {
    info!("Rescanning library");

    // Both the folder list and the recursion preference come from settings.
    // `recursive` used to be hardcoded `true` here, which quietly undid the
    // user's "Include subfolders" choice on every rescan even after
    // `add_game_folder` started honouring it -- so the two scan paths disagreed.
    let library_settings = crate::commands::settings::get_settings(state.clone())?.library;
    let folders = library_settings.folders;
    let recursive = library_settings.scan_recursive;

    let mut all_new_games = Vec::new();
    let mut all_errors = Vec::new();

    for folder in &folders {
        match scan_directory(folder.clone(), recursive) {
            Ok(result) => {
                all_errors.extend(result.errors);
                all_new_games.extend(result.games);
            }
            Err(e) => {
                warn!("Failed to rescan folder {}: {}", folder, e);
                all_errors.push(format!("Failed to scan {}: {}", folder, e));
            }
        }
    }

    let _library_guard = library_guard();

    let mut games = get_games_for_mutation()?;

    let existing_paths: std::collections::HashSet<_> = games.iter()
        .map(|g| normalize_path_for_comparison(&g.file_path))
        .collect();

    for game in all_new_games {
        if !existing_paths.contains(&normalize_path_for_comparison(&game.file_path)) {
            games.push(game);
        }
    }

    save_games(&games)?;

    let total = games.len();
    info!("Rescan complete. Total games: {}", total);

    Ok(ScanResult {
        games,
        total,
        errors: all_errors,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyResult {
    pub removed_count: usize,
    pub removed_titles: Vec<String>,
}

/// Splits `games` into (kept, removed) based on whether `file_path` still
/// exists on disk. Split out from the `#[tauri::command]` wrapper below so
/// it's unit-testable against an in-memory `Vec<Game>` (with real temp
/// files standing in for ROMs) without touching the real `library.json`.
fn partition_missing_games(games: Vec<Game>) -> (Vec<Game>, Vec<Game>) {
    games
        .into_iter()
        .partition(|g| std::path::Path::new(&g.file_path).exists())
}

/// Prunes any library entry whose `file_path` no longer exists on disk
/// (moved/deleted ROM), persists the pruned list, and reports what was
/// removed so the frontend can show a summary.
#[tauri::command]
pub fn verify_library() -> Result<VerifyResult, String> {
    info!("Verifying library");

    let _library_guard = library_guard();

    let games = get_games_for_mutation()?;
    let (kept, removed) = partition_missing_games(games);

    let removed_titles: Vec<String> = removed.iter().map(|g| g.title.clone()).collect();
    let removed_count = removed.len();

    if removed_count > 0 {
        save_games(&kept)?;
    }

    info!("Verify complete. Removed {} missing games", removed_count);

    Ok(VerifyResult {
        removed_count,
        removed_titles,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterCounts {
    pub regions: HashMap<String, u32>,
}

/// Counts games per region (`Game.country`). Split out from the
/// `#[tauri::command]` wrapper below so it's unit-testable against an
/// in-memory `Vec<Game>`. There is no genre field anywhere in `Game` or the
/// ROM header parsing, so genre counts are deliberately not part of this
/// shape -- inventing one would just show fake data in the UI.
fn compute_filter_counts(games: &[Game]) -> FilterCounts {
    let mut regions: HashMap<String, u32> = HashMap::new();
    for game in games {
        *regions.entry(game.country.clone()).or_insert(0) += 1;
    }
    FilterCounts { regions }
}

#[tauri::command]
pub fn get_filter_counts() -> Result<FilterCounts, String> {
    let games = get_games_for_mutation()?;
    Ok(compute_filter_counts(&games))
}

/// Returns the accumulated real-world play time (in seconds) for a game.
#[tauri::command]
pub fn get_game_play_time(game_id: String) -> Result<u64, String> {
    let games = get_games_for_mutation()?;
    let game = games
        .iter()
        .find(|g| g.id == game_id)
        .ok_or_else(|| format!("Game not found: {}", game_id))?;
    Ok(game.total_play_seconds)
}


#[tauri::command]
pub fn scan_directory(path: String, recursive: bool) -> Result<ScanResult, String> {
    info!("Scanning directory: {} (recursive: {})", path, recursive);

    let path = PathBuf::from(&path);
    if !path.exists() {
        return Err(format!("Directory does not exist: {:?}", path));
    }

    let mut games = Vec::new();
    let mut errors = Vec::new();

    let walker = if recursive {
        WalkDir::new(&path).follow_links(true)
    } else {
        WalkDir::new(&path).max_depth(1).follow_links(true)
    };

    for entry in walker.into_iter().filter_map(|e| e.ok()) {
        let entry_path = entry.path().to_path_buf();
        
        if entry_path.is_file() {
            // Check for ROM files
            if is_rom_file(&entry_path) {
                match parse_rom_file(&entry_path) {
                    Some(game) => {
                        debug!("Found ROM: {} ({})", game.title, game.rom_type);
                        games.push(game);
                    }
                    None => {
                        let err = format!("Failed to parse: {:?}", entry_path);
                        warn!("{}", err);
                        errors.push(err);
                    }
                }
            }
            // Check for archive files (ZIP)
            else if is_archive_file(&entry_path) {
                match parse_archive_file(&entry_path) {
                    Some(game) => {
                        debug!("Found ROM in archive: {} ({})", game.title, game.rom_type);
                        games.push(game);
                    }
                    None => {
                        let err = format!("Failed to extract ROM from archive: {:?}", entry_path);
                        warn!("{}", err);
                        errors.push(err);
                    }
                }
            }
        }
    }

    let total = games.len();
    info!("Scan complete: {} games found", total);

    Ok(ScanResult {
        games,
        total,
        errors,
    })
}
