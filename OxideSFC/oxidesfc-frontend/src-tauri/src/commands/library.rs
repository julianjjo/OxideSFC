use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tauri::State;
use tracing::{debug, info, warn};
use walkdir::WalkDir;

use crate::rom::{parse_rom_header, extract_rom_from_zip, ROM_EXTENSIONS};
use crate::AppState;

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    pub games: Vec<Game>,
    pub total: usize,
    pub errors: Vec<String>,
}

const ARCHIVE_EXTENSIONS: &[&str] = &["zip", "7z", "rar"];

/// Strips common release-group/region/revision markers (parentheses,
/// brackets, region names, revision tags) from a ROM filename stem to
/// derive a cleaner display title when the cartridge header itself doesn't
/// provide one. Shared by `parse_archive_file` and `parse_rom_file`, which
/// previously each inlined an identical copy of this logic.
fn clean_rom_title(filename: &str) -> String {
    filename
        .replace('(', "")
        .replace(')', "")
        .replace('[', "")
        .replace(']', "")
        .replace("USA", "")
        .replace("Europe", "")
        .replace("Japan", "")
        .replace("Rev", "")
        .replace("V1", "")
        .replace("V2", "")
        .trim()
        .to_string()
}

fn is_rom_file(path: &PathBuf) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| ROM_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

fn is_archive_file(path: &PathBuf) -> bool {
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
fn normalize_path_for_comparison(path: &str) -> String {
    match std::path::Path::new(path).canonicalize() {
        Ok(canonical) => canonical.to_string_lossy().to_lowercase(),
        Err(_) => path.to_lowercase(),
    }
}

/// Parse a ROM file from an archive (ZIP)
fn parse_archive_file(path: &PathBuf) -> Option<Game> {
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

fn parse_rom_file(path: &PathBuf) -> Option<Game> {
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

/// Like `get_games`, but propagates a hard error instead of silently
/// substituting an empty library when the file is corrupt. Used internally
/// by the read-modify-write mutation commands below: unlike the public
/// `get_games` read, treating "corrupt" as "empty" here would make the
/// subsequent `save_games` call permanently wipe out every previously
/// stored game.
fn get_games_for_mutation() -> Result<Vec<Game>, String> {
    let library_path = get_library_path()?;

    if !library_path.exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(&library_path)
        .map_err(|e| format!("Failed to read library: {}", e))?;

    serde_json::from_str(&content).map_err(|e| format!("Failed to parse library: {}", e))
}

#[tauri::command]
pub fn add_game_folder(path: String, state: State<AppState>) -> Result<ScanResult, String> {
    info!("Adding game folder: {}", path);

    // Scan the directory
    let result = scan_directory(path.clone(), true)?;

    // Hold the library lock for the entire get-modify-save sequence so a
    // concurrent add_game_folder/remove_game call can't interleave with
    // this one and lose an update.
    let _library_guard = state.library_lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

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
pub fn remove_game(game_id: String, state: State<AppState>) -> Result<(), String> {
    info!("Removing game: {}", game_id);

    let _library_guard = state.library_lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

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
pub fn toggle_game_favorite(game_id: String, state: State<AppState>) -> Result<bool, String> {
    info!("Toggling favorite for game: {}", game_id);

    let _library_guard = state.library_lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut games = get_games_for_mutation()?;
    let new_value = toggle_favorite_in(&mut games, &game_id)?;
    save_games(&games)?;

    Ok(new_value)
}

/// Wipes every entry from the library, without touching the underlying ROM
/// files on disk -- this only clears `library.json`.
#[tauri::command]
pub fn clear_library(state: State<AppState>) -> Result<(), String> {
    info!("Clearing library");

    let _library_guard = state.library_lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

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

    let folders = crate::commands::settings::get_settings(state.clone())?.library.folders;

    let mut all_new_games = Vec::new();
    let mut all_errors = Vec::new();

    for folder in &folders {
        match scan_directory(folder.clone(), true) {
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

    let _library_guard = state.library_lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

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
pub fn verify_library(state: State<AppState>) -> Result<VerifyResult, String> {
    info!("Verifying library");

    let _library_guard = state.library_lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

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

/// Guards `add_play_seconds_to_file`'s read-modify-write sequence.
/// `EmulationController` (which calls that function from `pause`/`stop`)
/// only has access to `library.json` via the filesystem, not via
/// `AppState.library_lock` -- `EmulationController` is itself owned inside
/// `AppState` behind its own `Mutex`, with no back-reference to the state
/// that contains it. A dedicated static mutex gives the same
/// read-modify-write safety as `library_lock` without needing that
/// back-reference.
static PLAY_TIME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Adds `seconds` of real-world elapsed time to a game's
/// `total_play_seconds` and persists the change. Called by
/// `EmulationController` on pause/stop, not exposed directly as a Tauri
/// command -- the frontend never needs to set play time itself.
pub fn add_play_seconds_to_file(game_id: &str, seconds: u64) -> Result<(), String> {
    if seconds == 0 {
        return Ok(());
    }

    let _guard = PLAY_TIME_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut games = get_games_for_mutation()?;
    if let Some(game) = games.iter_mut().find(|g| g.id == game_id) {
        game.total_play_seconds = game.total_play_seconds.saturating_add(seconds);
        save_games(&games)?;
    } else {
        warn!("add_play_seconds_to_file: game not found: {}", game_id);
    }

    Ok(())
}

fn get_library_path() -> Result<PathBuf, String> {
    let data_dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("OxideSFC");

    fs::create_dir_all(&data_dir)
        .map_err(|e| format!("Failed to create data directory: {}", e))?;

    Ok(data_dir.join("library.json"))
}

fn save_games(games: &[Game]) -> Result<(), String> {
    let library_path = get_library_path()?;

    let content = serde_json::to_string_pretty(games)
        .map_err(|e| format!("Failed to serialize library: {}", e))?;

    // Write to a temp file and rename into place instead of writing the
    // real path directly -- a crash or power loss mid-`fs::write` would
    // otherwise leave library.json truncated/corrupt. Rename is atomic on
    // the same filesystem, so readers always see either the old or the
    // fully-written new content, never a partial file.
    let tmp_path = library_path.with_extension("json.tmp");
    fs::write(&tmp_path, content)
        .map_err(|e| format!("Failed to write library: {}", e))?;
    fs::rename(&tmp_path, &library_path)
        .map_err(|e| format!("Failed to finalize library write: {}", e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        compute_filter_counts, normalize_path_for_comparison, partition_missing_games,
        toggle_favorite_in, Game,
    };
    use std::fs;

    /// Builds a minimal `Game` for the pure-logic tests below -- only the
    /// fields each test actually inspects need real values, everything else
    /// is a stable placeholder.
    fn make_game(id: &str, file_path: &str, country: &str) -> Game {
        Game {
            id: id.to_string(),
            title: format!("Game {}", id),
            file_path: file_path.to_string(),
            file_name: "game.smc".to_string(),
            file_size: 1024,
            rom_type: "LoROM".to_string(),
            sram_size: 0,
            country: country.to_string(),
            play_count: 0,
            last_played: None,
            favorite: false,
            custom_cover_path: None,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
            total_play_seconds: 0,
        }
    }

    /// A differently-cased path to a real file must normalize to the same
    /// value as the original, since Windows paths are case-insensitive --
    /// this is the exact duplicate-detection failure the bug report
    /// describes (re-adding a folder via a differently-cased path caused
    /// every game in it to be duplicated).
    #[test]
    fn canonicalizes_differently_cased_paths_to_the_same_value() {
        let dir = std::env::temp_dir().join(format!(
            "oxidesfc_test_normalize_path_case_{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).expect("create temp dir");
        let file_path = dir.join("Some Rom.sfc");
        fs::write(&file_path, b"test").expect("write temp file");

        let lower = file_path.to_string_lossy().to_lowercase();
        let upper = file_path.to_string_lossy().to_uppercase();

        assert_eq!(
            normalize_path_for_comparison(&lower),
            normalize_path_for_comparison(&upper),
            "differently-cased paths to the same real file should normalize identically"
        );

        fs::remove_dir_all(&dir).ok();
    }

    /// A trailing separator / `.` segment shouldn't change the normalized
    /// form for a path that actually exists.
    #[test]
    fn canonicalizes_paths_with_different_forms_to_the_same_value() {
        let dir = std::env::temp_dir().join(format!(
            "oxidesfc_test_normalize_path_form_{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).expect("create temp dir");
        let file_path = dir.join("Game.smc");
        fs::write(&file_path, b"test").expect("write temp file");

        let plain = file_path.to_string_lossy().to_string();
        let with_dot_segment = dir.join(".").join("Game.smc").to_string_lossy().to_string();

        assert_eq!(
            normalize_path_for_comparison(&plain),
            normalize_path_for_comparison(&with_dot_segment),
            "a `.` path segment shouldn't change the normalized form of an existing path"
        );

        fs::remove_dir_all(&dir).ok();
    }

    /// When the path no longer exists (canonicalize fails, e.g. the folder
    /// was deleted since the last scan), comparison must fall back to a
    /// raw-string comparison rather than erroring out of the whole
    /// add_game_folder call -- and that fallback should still be
    /// case-insensitive so it behaves consistently with the happy path.
    #[test]
    fn falls_back_to_lowercased_raw_comparison_for_nonexistent_paths() {
        let missing_lower = "z:\\definitely\\does\\not\\exist\\game.sfc";
        let missing_upper = "Z:\\DEFINITELY\\DOES\\NOT\\EXIST\\GAME.SFC";

        assert_eq!(
            normalize_path_for_comparison(missing_lower),
            normalize_path_for_comparison(missing_upper),
        );
        assert_eq!(
            normalize_path_for_comparison(missing_lower),
            missing_lower.to_lowercase()
        );
    }

    /// toggle_game_favorite's core logic: flips false -> true, returns the
    /// new value, and leaves every other game in the list untouched.
    #[test]
    fn toggle_favorite_in_flips_the_matching_game_and_returns_new_value() {
        let mut games = vec![
            make_game("a", "a.smc", "USA"),
            make_game("b", "b.smc", "USA"),
        ];

        let result = toggle_favorite_in(&mut games, "a").expect("game a exists");

        assert!(result, "toggling an initially-false favorite must return true");
        assert!(games[0].favorite, "game a must now be favorited");
        assert!(!games[1].favorite, "game b must be untouched");
    }

    /// A second toggle must flip back to false -- this is the exact
    /// rapid-double-toggle scenario the frontend fix (routing through
    /// libraryStore's fresh state instead of a stale closure value) exists
    /// to keep correct: two toggles in a row must cancel out, not both
    /// apply the same direction.
    #[test]
    fn toggle_favorite_in_twice_returns_to_the_original_value() {
        let mut games = vec![make_game("a", "a.smc", "USA")];

        let first = toggle_favorite_in(&mut games, "a").expect("game a exists");
        let second = toggle_favorite_in(&mut games, "a").expect("game a exists");

        assert!(first);
        assert!(!second, "toggling twice must return to the original (false) value");
    }

    #[test]
    fn toggle_favorite_in_errors_on_unknown_game_id() {
        let mut games = vec![make_game("a", "a.smc", "USA")];
        let result = toggle_favorite_in(&mut games, "does-not-exist");
        assert!(result.is_err(), "toggling a nonexistent game id must error, not silently no-op");
    }

    /// verify_library's core logic: a game whose file_path still exists on
    /// disk is kept; one whose file has been deleted/moved is removed and
    /// reported back by title.
    #[test]
    fn partition_missing_games_separates_existing_from_missing_files() {
        let dir = std::env::temp_dir().join(format!(
            "oxidesfc_test_partition_missing_{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).expect("create temp dir");
        let present_path = dir.join("present.smc");
        fs::write(&present_path, b"test").expect("write temp file");
        let missing_path = dir.join("missing.smc");
        // Deliberately not created -- simulates a ROM that was deleted/moved
        // since it was added to the library.

        let games = vec![
            make_game("present", &present_path.to_string_lossy(), "USA"),
            make_game("missing", &missing_path.to_string_lossy(), "USA"),
        ];

        let (kept, removed) = partition_missing_games(games);

        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].id, "present");
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].id, "missing");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn partition_missing_games_keeps_everything_when_all_files_exist() {
        let dir = std::env::temp_dir().join(format!(
            "oxidesfc_test_partition_all_present_{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("game.smc");
        fs::write(&path, b"test").expect("write temp file");

        let games = vec![make_game("a", &path.to_string_lossy(), "USA")];
        let (kept, removed) = partition_missing_games(games);

        assert_eq!(kept.len(), 1);
        assert!(removed.is_empty());

        fs::remove_dir_all(&dir).ok();
    }

    /// get_filter_counts' core logic: groups games by their exact `country`
    /// string and counts them -- case is preserved as-is (the frontend is
    /// responsible for any normalization it needs, e.g. FilterSidebar.tsx
    /// lowercasing these keys to match its own lowercase filter values).
    #[test]
    fn compute_filter_counts_groups_by_region() {
        let games = vec![
            make_game("a", "a.smc", "USA"),
            make_game("b", "b.smc", "USA"),
            make_game("c", "c.smc", "Japan"),
            make_game("d", "d.smc", "Europe"),
        ];

        let counts = compute_filter_counts(&games);

        assert_eq!(counts.regions.get("USA"), Some(&2));
        assert_eq!(counts.regions.get("Japan"), Some(&1));
        assert_eq!(counts.regions.get("Europe"), Some(&1));
        assert_eq!(counts.regions.get("Brazil"), None);
    }

    #[test]
    fn compute_filter_counts_on_empty_library_returns_empty_map() {
        let counts = compute_filter_counts(&[]);
        assert!(counts.regions.is_empty());
    }

    /// `total_play_seconds` must default to 0 when deserializing a
    /// `library.json` written before the field existed -- this is exactly
    /// what real pre-existing library.json files on disk look like, so a
    /// missing field must not fail the whole parse.
    #[test]
    fn game_deserializes_with_missing_total_play_seconds_defaulting_to_zero() {
        let json = r#"{
            "id": "a",
            "title": "Game A",
            "file_path": "a.smc",
            "file_name": "a.smc",
            "file_size": 1024,
            "rom_type": "LoROM",
            "sram_size": 0,
            "country": "USA",
            "play_count": 0,
            "last_played": null,
            "favorite": false,
            "custom_cover_path": null,
            "created_at": "2024-01-01T00:00:00Z",
            "updated_at": "2024-01-01T00:00:00Z"
        }"#;

        let game: Game = serde_json::from_str(json).expect("must deserialize without total_play_seconds");
        assert_eq!(game.total_play_seconds, 0);
    }
}
