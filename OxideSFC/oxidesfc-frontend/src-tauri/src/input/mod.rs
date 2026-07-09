mod gamepad;
mod keyboard;

pub use gamepad::InputManager;

use crate::AppState;
use tauri::State;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InputEvent {
    pub event_type: String,
    pub button: Option<String>,
    pub value: Option<f32>,
    pub gamepad_id: Option<String>,
}

#[tauri::command]
pub fn poll_gamepad_events(state: State<AppState>) -> Result<Vec<InputEvent>, String> {
    let mut input_manager = state.input_manager.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    Ok(input_manager.poll_events())
}
