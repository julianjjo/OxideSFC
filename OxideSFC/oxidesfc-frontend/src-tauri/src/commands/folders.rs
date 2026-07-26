use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::State;
use tracing::{info, warn};

use crate::commands::library::{get_games, Game};
use crate::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameFolder {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub created_at: String,
    /// Number of games currently associated with this folder. Computed on
    /// read from the associations list -- not stored -- so it can never
    /// drift out of sync with the actual membership. `CollectionFolders.tsx`
    /// displays this next to each folder's name.
    #[serde(default)]
    pub game_count: usize,
}

/// The full persisted shape of `folders.json`: the folders themselves, plus
/// a simple many-to-many (game_id, folder_id) association list. This is
/// intentionally not relationally sophisticated -- a `Vec` of pairs is
/// enough for the folder counts/membership queries the UI needs, and keeps
/// this file's shape a direct mirror of `library.json`'s "just a Vec,
/// atomically written" pattern.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct FoldersStore {
    #[serde(default)]
    folders: Vec<GameFolder>,
    /// (game_id, folder_id) pairs.
    #[serde(default)]
    associations: Vec<(String, String)>,
}

fn get_folders_path() -> Result<PathBuf, String> {
    let data_dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("OxideSFC");

    fs::create_dir_all(&data_dir)
        .map_err(|e| format!("Failed to create data directory: {}", e))?;

    Ok(data_dir.join("folders.json"))
}

fn load_store() -> Result<FoldersStore, String> {
    let path = get_folders_path()?;

    if !path.exists() {
        return Ok(FoldersStore::default());
    }

    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read folders: {}", e))?;

    match serde_json::from_str(&content) {
        Ok(store) => Ok(store),
        Err(e) => {
            warn!("Failed to parse folders file, falling back to empty: {}", e);
            Ok(FoldersStore::default())
        }
    }
}

/// Like `load_store`, but propagates a hard error instead of silently
/// substituting an empty store when the file is corrupt -- used by mutation
/// commands so a corrupt-but-recoverable file doesn't get permanently wiped
/// out by the subsequent save.
fn load_store_for_mutation() -> Result<FoldersStore, String> {
    let path = get_folders_path()?;

    if !path.exists() {
        return Ok(FoldersStore::default());
    }

    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read folders: {}", e))?;

    serde_json::from_str(&content).map_err(|e| format!("Failed to parse folders: {}", e))
}

fn save_store(store: &FoldersStore) -> Result<(), String> {
    let path = get_folders_path()?;

    let content = serde_json::to_string_pretty(store)
        .map_err(|e| format!("Failed to serialize folders: {}", e))?;

    // Same atomic write-then-rename pattern as library.rs/settings.rs's
    // save helpers -- avoids leaving folders.json truncated/corrupt if the
    // process dies mid-write.
    let tmp_path = path.with_extension("json.tmp");
    fs::write(&tmp_path, content)
        .map_err(|e| format!("Failed to write folders: {}", e))?;
    fs::rename(&tmp_path, &path)
        .map_err(|e| format!("Failed to finalize folders write: {}", e))?;

    Ok(())
}

#[tauri::command]
pub fn get_folders() -> Result<Vec<GameFolder>, String> {
    let store = load_store()?;
    let mut folders = store.folders;
    for folder in &mut folders {
        folder.game_count = store
            .associations
            .iter()
            .filter(|(_, fid)| fid == &folder.id)
            .count();
    }
    Ok(folders)
}

#[tauri::command]
pub fn get_games_in_folder(folder_id: String) -> Result<Vec<Game>, String> {
    let store = load_store()?;
    let game_ids: std::collections::HashSet<&String> = store
        .associations
        .iter()
        .filter(|(_, fid)| fid == &folder_id)
        .map(|(gid, _)| gid)
        .collect();

    let games = get_games()?;
    Ok(games
        .into_iter()
        .filter(|g| game_ids.contains(&g.id))
        .collect())
}

/// Ids of the collections a single game belongs to.
///
/// The inverse of `get_games_in_folder`, and the query the game details panel
/// needs: it has to show membership for every collection at once, which would
/// otherwise mean one `get_games_in_folder` round-trip per collection (each of
/// which also loads the whole library) just to render a list of checkboxes.
#[tauri::command]
pub fn get_folders_for_game(game_id: String) -> Result<Vec<String>, String> {
    let store = load_store()?;
    Ok(store
        .associations
        .iter()
        .filter(|(gid, _)| gid == &game_id)
        .map(|(_, fid)| fid.clone())
        .collect())
}

#[tauri::command]
pub fn create_folder(name: String, state: State<AppState>) -> Result<GameFolder, String> {
    info!("Creating folder: {}", name);

    let _guard = state.folders_lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut store = load_store_for_mutation()?;

    let folder = GameFolder {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        parent_id: None,
        created_at: chrono::Utc::now().to_rfc3339(),
        game_count: 0,
    };

    store.folders.push(folder.clone());
    save_store(&store)?;

    Ok(folder)
}

#[tauri::command]
pub fn rename_folder(folder_id: String, name: String, state: State<AppState>) -> Result<(), String> {
    info!("Renaming folder {} to {}", folder_id, name);

    let _guard = state.folders_lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut store = load_store_for_mutation()?;
    let folder = store
        .folders
        .iter_mut()
        .find(|f| f.id == folder_id)
        .ok_or_else(|| format!("Folder not found: {}", folder_id))?;
    folder.name = name;
    save_store(&store)?;

    Ok(())
}

#[tauri::command]
pub fn delete_folder(folder_id: String, state: State<AppState>) -> Result<(), String> {
    info!("Deleting folder: {}", folder_id);

    let _guard = state.folders_lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut store = load_store_for_mutation()?;
    store.folders.retain(|f| f.id != folder_id);
    // Also drop any game-folder associations pointing at the now-deleted
    // folder, so they don't linger as orphaned references.
    store.associations.retain(|(_, fid)| fid != &folder_id);
    save_store(&store)?;

    Ok(())
}

#[tauri::command]
pub fn add_game_to_folder(game_id: String, folder_id: String, state: State<AppState>) -> Result<(), String> {
    info!("Adding game {} to folder {}", game_id, folder_id);

    let _guard = state.folders_lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut store = load_store_for_mutation()?;

    if !store.folders.iter().any(|f| f.id == folder_id) {
        return Err(format!("Folder not found: {}", folder_id));
    }

    let pair = (game_id, folder_id);
    if !store.associations.contains(&pair) {
        store.associations.push(pair);
        save_store(&store)?;
    }

    Ok(())
}

#[tauri::command]
pub fn remove_game_from_folder(game_id: String, folder_id: String, state: State<AppState>) -> Result<(), String> {
    info!("Removing game {} from folder {}", game_id, folder_id);

    let _guard = state.folders_lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

    let mut store = load_store_for_mutation()?;
    store.associations.retain(|(gid, fid)| !(gid == &game_id && fid == &folder_id));
    save_store(&store)?;

    Ok(())
}
