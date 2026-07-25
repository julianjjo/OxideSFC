//! Library tests: title cleanup, path normalization, scanning, and the
//! pure helpers behind the mutation commands.


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
