//! Reading and writing `library.json`, plus the play-time accumulator that
//! updates it from outside the Tauri command layer.

use super::Game;
use std::fs;
use std::path::PathBuf;
use tracing::warn;

/// Like `get_games`, but propagates a hard error instead of silently
/// substituting an empty library when the file is corrupt. Used internally
/// by the read-modify-write mutation commands below: unlike the public
/// `get_games` read, treating "corrupt" as "empty" here would make the
/// subsequent `save_games` call permanently wipe out every previously
/// stored game.
pub(super) fn get_games_for_mutation() -> Result<Vec<Game>, String> {
    let library_path = get_library_path()?;

    if !library_path.exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(&library_path)
        .map_err(|e| format!("Failed to read library: {}", e))?;

    serde_json::from_str(&content).map_err(|e| format!("Failed to parse library: {}", e))
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

/// Records the start of a play session: bumps `play_count` and stamps
/// `last_played`.
///
/// These two fields existed on `Game`, were serialised, and were read by the
/// library UI -- but nothing ever wrote them, so both stayed at their scan-time
/// defaults (0 and null) forever. Only `total_play_seconds` was being recorded.
/// The visible effect was that "Continue" and "Recently played" could never show
/// anything, and the library's play-count and last-played columns were
/// permanently blank no matter how much a game had been played.
///
/// Shares `PLAY_TIME_LOCK` with `add_play_seconds_to_file` for the same reason:
/// both are read-modify-write cycles over `library.json` reached from
/// `EmulationController`, which has no access to `AppState.library_lock`.
pub fn record_play_start(game_id: &str) -> Result<(), String> {
    let _guard = PLAY_TIME_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut games = get_games_for_mutation()?;
    if let Some(game) = games.iter_mut().find(|g| g.id == game_id) {
        game.play_count = game.play_count.saturating_add(1);
        game.last_played = Some(chrono::Utc::now().to_rfc3339());
        game.updated_at = chrono::Utc::now().to_rfc3339();
        save_games(&games)?;
    } else {
        warn!("record_play_start: game not found: {}", game_id);
    }

    Ok(())
}

/// One library entry by id, or `None` if it is not in the library.
///
/// Read-only, so it takes no lock: a torn read cannot happen because
/// `save_games` writes through a temp file and renames, so a reader sees either
/// the whole old file or the whole new one.
pub fn get_game_by_id(game_id: &str) -> Result<Option<Game>, String> {
    let games = get_games_for_mutation()?;
    Ok(games.into_iter().find(|g| g.id == game_id))
}

/// Point a library entry at (or away from) a cover image.
///
/// Shares `PLAY_TIME_LOCK` with the other `library.json` read-modify-write
/// helpers reached from outside `AppState`: cover fetching runs several games
/// concurrently, so without a shared lock two finishing downloads would each
/// write back a copy of the file read before the other's update and one would be
/// lost.
pub fn set_cover_file(game_id: &str, cover_file: Option<String>) -> Result<(), String> {
    let _guard = PLAY_TIME_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut games = get_games_for_mutation()?;
    if let Some(game) = games.iter_mut().find(|g| g.id == game_id) {
        if game.cover_file == cover_file {
            return Ok(());
        }
        game.cover_file = cover_file;
        game.updated_at = chrono::Utc::now().to_rfc3339();
        save_games(&games)?;
    } else {
        warn!("set_cover_file: game not found: {}", game_id);
    }

    Ok(())
}

/// Set `cover_file` on every entry at once, for cache-wide operations.
pub fn set_cover_file_for_all(cover_file: Option<String>) -> Result<(), String> {
    let _guard = PLAY_TIME_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut games = get_games_for_mutation()?;
    let now = chrono::Utc::now().to_rfc3339();
    let mut changed = false;
    for game in games.iter_mut() {
        if game.cover_file != cover_file {
            game.cover_file = cover_file.clone();
            game.updated_at = now.clone();
            changed = true;
        }
    }

    if changed {
        save_games(&games)?;
    }

    Ok(())
}

pub(super) fn get_library_path() -> Result<PathBuf, String> {
    let data_dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("OxideSFC");

    fs::create_dir_all(&data_dir)
        .map_err(|e| format!("Failed to create data directory: {}", e))?;

    Ok(data_dir.join("library.json"))
}

pub(super) fn save_games(games: &[Game]) -> Result<(), String> {
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

