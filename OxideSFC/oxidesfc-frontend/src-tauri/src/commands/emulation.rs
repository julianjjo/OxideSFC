use crate::emulation::{GameInfo, InputState, VideoFrame};
use crate::AppState;
use tauri::State;
use tracing::{debug, info};

#[tauri::command]
pub fn load_rom(path: String, state: State<AppState>) -> Result<GameInfo, String> {
    info!("Loading ROM: {}", path);
    let mut emulation = state.emulation.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    emulation.load_rom(&path)
}

#[tauri::command]
pub fn start_emulation(state: State<AppState>, game_id: Option<String>) -> Result<(), String> {
    info!("Starting emulation (game_id: {:?})", game_id);
    let mut emulation = state.emulation.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    emulation.start(game_id)
}

#[tauri::command]
pub fn pause_emulation(state: State<AppState>) -> Result<(), String> {
    info!("Pausing emulation");
    let mut emulation = state.emulation.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    emulation.pause()
}

#[tauri::command]
pub fn resume_emulation(state: State<AppState>) -> Result<(), String> {
    info!("Resuming emulation");
    let mut emulation = state.emulation.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    emulation.resume()
}

#[tauri::command]
pub fn stop_emulation(state: State<AppState>) -> Result<(), String> {
    info!("Stopping emulation");
    let mut emulation = state.emulation.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    emulation.stop()
}

/// Advances the paced emulation, then returns the rendered frame -- or
/// `None` when no new emulated frame completed since the previous call.
/// The frontend polls at monitor refresh rate (which can be 144/240Hz)
/// while NTSC produces ~60 frames/sec; returning `None` for the
/// in-between polls skips a ~230KB clone + base64 encode per call, load
/// that competed with emulation stepping and starved the audio pipeline.
#[tauri::command]
pub fn get_video_frame(state: State<AppState>) -> Result<Option<VideoFrame>, String> {
    let mut emulation = state.emulation.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    emulation.step_frame();
    Ok(emulation.poll_frame())
}

/// Drains the buffered PCM as raw little-endian `i16` bytes (interleaved
/// L0, R0, L1, R1, ...) through Tauri's binary IPC path. Returning
/// `Vec<i16>` here would serialize every sample into a JSON number array
/// (and re-parse it in JS) on every animation frame; `tauri::ipc::Response`
/// hands the frontend an `ArrayBuffer` it can view as an `Int16Array`
/// with no per-sample encoding at all.
#[tauri::command]
pub fn get_audio_samples(state: State<AppState>) -> tauri::ipc::Response {
    let mut emulation = state.emulation.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let samples = emulation.get_audio();
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    tauri::ipc::Response::new(bytes)
}

/// Sets the emulation speed multiplier (1.0 = real NTSC speed; clamped to
/// 0.1-4.0 by the controller). Returns the value actually applied.
#[tauri::command]
pub fn set_emulation_speed(speed: f64, state: State<AppState>) -> Result<f64, String> {
    let mut emulation = state.emulation.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    emulation.set_speed(speed);
    Ok(emulation.get_speed())
}

#[tauri::command]
pub fn get_emulation_speed(state: State<AppState>) -> Result<f64, String> {
    let emulation = state.emulation.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    Ok(emulation.get_speed())
}

#[tauri::command]
pub fn set_input_state(input: InputState, state: State<AppState>) -> Result<(), String> {
    debug!("Setting input state: {:?}", input);
    let mut emulation = state.emulation.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    emulation.set_input(input);
    Ok(())
}

#[tauri::command]
pub fn save_state(slot: u8, state: State<AppState>) -> Result<(), String> {
    info!("Saving state to slot {}", slot);
    let emulation = state.emulation.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    emulation.save_state(slot)
}

#[tauri::command]
pub fn load_state(slot: u8, state: State<AppState>) -> Result<(), String> {
    info!("Loading state from slot {}", slot);
    let mut emulation = state.emulation.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    emulation.load_state(slot)
}

#[tauri::command]
pub fn get_game_info(state: State<AppState>) -> Result<Option<GameInfo>, String> {
    let emulation = state.emulation.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    Ok(emulation.get_game_info())
}

/// Reports whether the CPU has permanently halted (e.g. hit an
/// unimplemented opcode while executing) and, if so, why. Distinct from
/// "paused" -- a paused emulation can be resumed, a halted one can't (short
/// of loading a new ROM). Nothing in the frontend surfaces this yet; that's
/// a follow-up UI task, not part of this command.
#[tauri::command]
pub fn get_halt_status(state: State<AppState>) -> Result<Option<String>, String> {
    let emulation = state.emulation.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    Ok(emulation.halt_reason())
}
