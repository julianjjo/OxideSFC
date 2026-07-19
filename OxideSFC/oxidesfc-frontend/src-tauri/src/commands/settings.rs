use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tauri::State;
use tracing::{debug, info, warn};

use crate::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoSettings {
    pub vsync: bool,
    pub frame_limit: String,
    pub renderer: String,
    pub shader: String,
    pub scale_mode: String,
}

impl Default for VideoSettings {
    fn default() -> Self {
        Self {
            vsync: true,
            frame_limit: "60".to_string(),
            renderer: "webgl".to_string(),
            shader: "none".to_string(),
            // Nearest keeps the SNES's 256x224 pixels crisp when scaled up;
            // bilinear visibly blurs sprites and tile edges.
            scale_mode: "nearest".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSettings {
    pub enabled: bool,
    pub volume: f32,
    pub latency: u32,
    // `#[serde(default)]` so settings.json files saved before these fields
    // existed still deserialize (missing fields fall back to their Default
    // impl below instead of failing the whole parse).
    #[serde(default = "default_sfx_volume")]
    pub sfx_volume: f32,
    #[serde(default = "default_music_volume")]
    pub music_volume: f32,
    #[serde(default = "default_buffering_enabled")]
    pub buffering_enabled: bool,
}

fn default_sfx_volume() -> f32 {
    100.0
}

fn default_music_volume() -> f32 {
    100.0
}

fn default_buffering_enabled() -> bool {
    true
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            volume: 1.0,
            latency: 50,
            sfx_volume: default_sfx_volume(),
            music_volume: default_music_volume(),
            buffering_enabled: default_buffering_enabled(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlSettings {
    pub keyboard_enabled: bool,
    pub gamepad_enabled: bool,
    pub keyboard_mapping: HashMap<String, String>,
    pub gamepad_profile: String,
}

impl Default for ControlSettings {
    fn default() -> Self {
        let mut mapping = HashMap::new();
        
        // Default keyboard mapping
        mapping.insert("ArrowUp".to_string(), "up".to_string());
        mapping.insert("ArrowDown".to_string(), "down".to_string());
        mapping.insert("ArrowLeft".to_string(), "left".to_string());
        mapping.insert("ArrowRight".to_string(), "right".to_string());
        mapping.insert("KeyZ".to_string(), "a".to_string());
        mapping.insert("KeyX".to_string(), "b".to_string());
        mapping.insert("KeyA".to_string(), "x".to_string());
        mapping.insert("KeyS".to_string(), "y".to_string());
        mapping.insert("KeyQ".to_string(), "l".to_string());
        mapping.insert("KeyW".to_string(), "r".to_string());
        mapping.insert("Enter".to_string(), "start".to_string());
        mapping.insert("ShiftRight".to_string(), "select".to_string());

        Self {
            keyboard_enabled: true,
            gamepad_enabled: true,
            keyboard_mapping: mapping,
            gamepad_profile: "default".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibrarySettings {
    pub folders: Vec<String>,
    pub scan_recursive: bool,
    pub use_metadata: bool,
    pub cover_resolution: String,
    /// Preferred metadata/artwork provider (e.g. "local", "screenscraper",
    /// "igdb", "openvgdb" -- see `LibrarySettings.tsx`'s
    /// `ARTWORK_SOURCE_OPTIONS`). `#[serde(default)]` so settings.json files
    /// written before this field existed still deserialize.
    #[serde(default = "default_artwork_source")]
    pub artwork_source: String,
}

fn default_artwork_source() -> String {
    "screenscraper".to_string()
}

impl Default for LibrarySettings {
    fn default() -> Self {
        Self {
            folders: Vec::new(),
            scan_recursive: true,
            use_metadata: true,
            cover_resolution: "medium".to_string(),
            artwork_source: default_artwork_source(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralSettings {
    pub language: String,
    pub theme: String,
    pub show_window_on_start: bool,
    pub confirm_on_exit: bool,
    #[serde(default)]
    pub has_completed_onboarding: bool,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            language: "en".to_string(),
            theme: "dark".to_string(),
            show_window_on_start: true,
            confirm_on_exit: true,
            has_completed_onboarding: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub general: GeneralSettings,
    pub video: VideoSettings,
    pub audio: AudioSettings,
    pub controls: ControlSettings,
    pub library: LibrarySettings,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            general: GeneralSettings::default(),
            video: VideoSettings::default(),
            audio: AudioSettings::default(),
            controls: ControlSettings::default(),
            library: LibrarySettings::default(),
        }
    }
}

fn get_settings_path_internal() -> Result<PathBuf, String> {
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("OxideSFC");

    fs::create_dir_all(&config_dir)
        .map_err(|e| format!("Failed to create config directory: {}", e))?;

    Ok(config_dir.join("settings.json"))
}

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Result<AppSettings, String> {
    debug!("Loading settings");

    // Hold the settings lock for the whole "check exists -> maybe
    // bootstrap defaults -> read" sequence, mirroring commands::library's
    // library_lock pattern, so a concurrent save_settings can't sneak a
    // write in between this read and a default-bootstrap save below.
    let _settings_guard = state
        .settings_lock
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let settings_path = get_settings_path_internal()?;

    if !settings_path.exists() {
        // Return default settings
        let default_settings = AppSettings::default();
        // Save default settings
        save_settings_locked(&default_settings)?;
        return Ok(default_settings);
    }

    let content = fs::read_to_string(&settings_path)
        .map_err(|e| format!("Failed to read settings: {}", e))?;

    match serde_json::from_str(&content) {
        Ok(settings) => Ok(settings),
        Err(e) => {
            // The file exists but isn't valid JSON (e.g. truncated by a
            // crash mid-write). Recover the same way a missing file is
            // handled rather than surfacing a hard error to the UI.
            warn!("Failed to parse settings file, falling back to defaults: {}", e);
            Ok(AppSettings::default())
        }
    }
}

#[tauri::command]
pub fn save_settings(settings: AppSettings, state: State<AppState>) -> Result<(), String> {
    info!("Saving settings");

    // Guard the write with the same lock get_settings takes around its
    // load-then-maybe-save sequence. Without this, two near-simultaneous
    // save_settings calls (or a save racing get_settings' default-bootstrap
    // save) can race: both read/build their in-memory settings, then write
    // in some order, and the second write silently clobbers the first's
    // changes (last-writer-wins on top of a stale read). Serializing the
    // writes here ensures they apply atomically with respect to each other.
    let _settings_guard = state
        .settings_lock
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    save_settings_locked(&settings)
}

/// Actual settings-file write, factored out so callers that already hold
/// `settings_lock` (get_settings' default-bootstrap path) don't try to
/// re-acquire the same non-reentrant `Mutex` and deadlock.
fn save_settings_locked(settings: &AppSettings) -> Result<(), String> {
    let settings_path = get_settings_path_internal()?;

    let content = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;

    // Write to a temp file and rename into place instead of writing the
    // real path directly -- a crash or power loss mid-`fs::write` would
    // otherwise leave settings.json truncated/corrupt. Rename is atomic on
    // the same filesystem, so readers always see either the old or the
    // fully-written new content, never a partial file.
    let tmp_path = settings_path.with_extension("json.tmp");
    fs::write(&tmp_path, content)
        .map_err(|e| format!("Failed to write settings: {}", e))?;
    fs::rename(&tmp_path, &settings_path)
        .map_err(|e| format!("Failed to finalize settings write: {}", e))?;

    Ok(())
}

#[tauri::command]
pub fn get_settings_path() -> Result<String, String> {
    let path = get_settings_path_internal()?;
    Ok(path.to_string_lossy().to_string())
}
