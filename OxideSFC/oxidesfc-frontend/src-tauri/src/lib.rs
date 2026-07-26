mod commands;
mod emulation;
mod input;
mod platform;
mod rom;

use std::sync::Mutex;
use tracing::{error, info};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

// Global application state
pub struct AppState {
    pub emulation: Mutex<emulation::EmulationController>,
    pub input_manager: Mutex<input::InputManager>,
    // NOTE: there is deliberately no `library_lock` here.
    //
    // `library.json` is guarded by a single static in `commands::library::store`
    // (acquired via `library_guard()`). It cannot live on `AppState`, because
    // `EmulationController` -- which records play sessions and play time -- is
    // itself owned inside this struct behind its own `Mutex` and has no
    // back-reference to reach a sibling field. Having a lock here *as well*
    // meant two mutexes guarding one file, which is not synchronisation: a save
    // taken under one could interleave with a save under the other and drop an
    // update. If you add a `library.json` writer, use `library_guard()`.
    /// Guards commands::settings' load-then-save sequences so two
    /// near-simultaneous save_settings invocations (or a save racing the
    /// default-settings bootstrap in get_settings) can't interleave a stale read
    /// with a write and silently drop one side's changes. Holds no data itself.
    pub settings_lock: Mutex<()>,
    /// Same pattern again, but for folders.json (commands::folders' game
    /// folders/collections store). Holds no data itself.
    pub folders_lock: Mutex<()>,
}

fn init_logging() -> tracing_appender::non_blocking::WorkerGuard {
    let log_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("OxideSFC")
        .join("logs");

    std::fs::create_dir_all(&log_dir).ok();

    let file_appender = tracing_appender::rolling::daily(&log_dir, "oxidesfc.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::registry()
        .with(
            fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false)
                .with_target(true)
                .with_thread_ids(true),
        )
        .with(fmt::layer().with_writer(std::io::stdout).with_ansi(true))
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    info!("OxideSFC logging initialized");

    guard
}

fn init_panic_handler() {
    std::panic::set_hook(Box::new(|panic_info| {
        let location = panic_info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown".to_string());

        let message = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic".to_string()
        };

        error!(target: "crash", location = %location, message = %message, "Application panicked");

        // Write crash report to file
        let crash_dir = dirs::data_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("OxideSFC")
            .join("crashes");

        std::fs::create_dir_all(&crash_dir).ok();

        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        let crash_file = crash_dir.join(format!("crash_{}.log", timestamp));

        let crash_content = format!(
            "OxideSFC Crash Report\n\
             ====================\n\
             Timestamp: {}\n\
             Location: {}\n\
             Message: {}\n\
             \n\
             Backtrace:\n{:?}",
            chrono::Utc::now(),
            location,
            message,
            std::backtrace::Backtrace::capture()
        );

        std::fs::write(&crash_file, crash_content).ok();
    }));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Must stay alive for the whole process lifetime: dropping this guard
    // shuts down the non-blocking file writer's background thread, which
    // silently kills file logging after this function returns the guard
    // (previously bound to a local `_guard` in `init_logging`, which was
    // dropped as soon as that function returned -- file logging worked
    // only until the very next log flush).
    let _log_guard = init_logging();
    init_panic_handler();

    info!("Starting OxideSFC frontend");

    let app_state = AppState {
        emulation: Mutex::new(emulation::EmulationController::new()),
        input_manager: Mutex::new(input::InputManager::new()),
        settings_lock: Mutex::new(()),
        folders_lock: Mutex::new(()),
    };

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init());

    // Shadowed under `cfg` rather than reassigned into a `mut` binding, so a
    // release build (where this plugin is absent) doesn't warn about a `mut`
    // it never needs.
    #[cfg(debug_assertions)]
    let builder = builder.plugin(tauri_plugin_mcp_bridge::init());

    builder
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            commands::emulation::load_rom,
            commands::emulation::start_emulation,
            commands::emulation::pause_emulation,
            commands::emulation::resume_emulation,
            commands::emulation::stop_emulation,
            commands::emulation::get_video_frame,
            commands::emulation::get_audio_samples,
            commands::emulation::set_emulation_speed,
            commands::emulation::get_emulation_speed,
            commands::emulation::set_input_state,
            commands::emulation::save_state,
            commands::emulation::load_state,
            commands::emulation::list_save_states,
            commands::emulation::get_game_info,
            commands::emulation::get_halt_status,
            commands::library::scan_directory,
            commands::library::get_games,
            commands::library::add_game_folder,
            commands::library::remove_game,
            commands::library::toggle_game_favorite,
            commands::library::clear_library,
            commands::library::rescan_library,
            commands::library::verify_library,
            commands::library::get_filter_counts,
            commands::library::get_game_play_time,
            commands::covers::get_covers_dir,
            commands::covers::fetch_cover,
            commands::covers::clear_cover_cache,
            commands::folders::get_folders,
            commands::folders::get_games_in_folder,
            commands::folders::get_folders_for_game,
            commands::folders::create_folder,
            commands::folders::rename_folder,
            commands::folders::delete_folder,
            commands::folders::add_game_to_folder,
            commands::folders::remove_game_from_folder,
            commands::settings::get_settings,
            commands::settings::save_settings,
            commands::settings::get_settings_path,
            input::poll_gamepad_events,
        ])
        .setup(|_app| {
            info!("OxideSFC setup complete");

            // Initialize platform-specific configuration
            let platform_config = platform::init_platform();
            info!("Platform: {:?}", platform_config);

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
