//! Cover art acquisition.
//!
//! Two sources, tried in order, both keyed off the ROM's file name:
//!
//! 1. **Local files.** Art sitting beside the ROM, or in a conventional sibling
//!    folder (`covers/`, `media/`, `boxart/`, `Named_Boxarts/`). Costs nothing,
//!    works offline, and lets anyone with a curated set keep using it.
//! 2. **The Libretro thumbnail CDN.** A public, GitHub-backed host with no API
//!    key, no account and no quota -- the same source RetroArch itself uses.
//!
//! ## Why not ScreenScraper or IGDB as the default
//!
//! Both need credentials this project cannot ship: ScreenScraper issues
//! per-application developer IDs, and IGDB requires a Twitch client *secret*,
//! which is not a secret once it is inside a desktop binary. Either would have to
//! be supplied by the user. They remain worth adding later as an opt-in tier,
//! matched on the ROM's CRC32 rather than its name, but they cannot be the
//! out-of-the-box path.
//!
//! ## Matching
//!
//! Libretro names its thumbnails after the No-Intro release name, which is what
//! ROM file names already are in practice -- `Donkey Kong Country (USA) (Rev
//! 1).sfc` maps straight onto `Donkey Kong Country (USA) (Rev 1).png`. That also
//! means archives work unchanged: what matters is the name of the file on disk,
//! not what is inside it.
//!
//! Name matching does mean a renamed or non-standard dump will miss. That is the
//! honest failure mode for this tier, and it is recorded (see the `.miss`
//! markers) so a miss costs one request ever rather than one per launch.

use serde::Serialize;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::{debug, info, warn};

use super::library::{get_game_by_id, set_cover_file, set_cover_file_for_all};

const LIBRETRO_BASE: &str = "https://thumbnails.libretro.com";
/// Libretro's directory name for this system, spelled exactly as the CDN has it.
const LIBRETRO_SYSTEM: &str = "Nintendo - Super Nintendo Entertainment System";
/// Boxarts rather than `Named_Titles`/`Named_Snaps`: the library grid is a shelf
/// of boxed software, so the box is the right image for it.
const LIBRETRO_KIND: &str = "Named_Boxarts";

/// Image extensions recognised when looking for local art.
const IMAGE_EXTENSIONS: [&str; 3] = ["png", "jpg", "jpeg"];

/// Sibling folders conventionally used for box art next to a ROM collection.
const LOCAL_ART_DIRS: [&str; 5] = ["covers", "media", "boxart", "box", "Named_Boxarts"];

/// Ceiling on a downloaded image, so a misbehaving host cannot stream unbounded
/// data into the covers directory. Libretro boxarts run a few hundred KB.
const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;

/// Where this game's cover came from. Surfaced so the UI can distinguish "no art
/// exists for this ROM" from "the download failed, try again later".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CoverSource {
    /// Already in the covers directory from an earlier run.
    Cache,
    /// Copied in from a file next to the ROM.
    Local,
    /// Downloaded from the Libretro thumbnail CDN.
    Libretro,
    /// No art found. Recorded so it is not retried on every launch.
    Missing,
    /// The lookup could not complete (offline, timeout, host error). NOT
    /// recorded as a miss, because it says nothing about whether art exists.
    Unavailable,
}

#[derive(Debug, Clone, Serialize)]
pub struct CoverResult {
    pub game_id: String,
    /// Absolute path to the image, when there is one.
    pub path: Option<String>,
    /// Bare file name, as stored in `Game.cover_file`.
    pub file: Option<String>,
    pub source: CoverSource,
}

/// The app's covers directory, created on demand.
pub(crate) fn covers_dir() -> Result<PathBuf, String> {
    let dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("OxideSFC")
        .join("covers");

    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create covers directory: {}", e))?;

    Ok(dir)
}

#[tauri::command]
pub fn get_covers_dir() -> Result<String, String> {
    Ok(covers_dir()?.to_string_lossy().to_string())
}

/// The release name a ROM file corresponds to: its file name minus the extension.
fn release_name(file_name: &str) -> String {
    Path::new(file_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(file_name)
        .to_string()
}

/// Libretro replaces characters that are illegal in file names on some platforms
/// with underscores when it names a thumbnail. Everything else -- including the
/// commas and parentheses that are all over No-Intro names -- is kept verbatim.
fn libretro_name(release: &str) -> String {
    release
        .chars()
        .map(|c| match c {
            '&' | '*' | '/' | ':' | '`' | '<' | '>' | '?' | '\\' | '|' | '"' => '_',
            other => other,
        })
        .collect()
}

/// Percent-encode one URL path segment.
///
/// Everything outside RFC 3986's unreserved set is escaped. That is stricter
/// than strictly necessary (parentheses and commas are legal in a path) but it is
/// unconditionally correct, which matters more here than producing the prettiest
/// URL -- these names contain spaces, brackets, commas and apostrophes.
fn encode_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len() * 3);
    for byte in segment.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(*byte as char)
            }
            other => out.push_str(&format!("%{:02X}", other)),
        }
    }
    out
}

/// GoodSNES-style single-letter region codes and their No-Intro spellings.
///
/// Libretro names its thumbnails after No-Intro releases, but a great many real
/// collections are GoodSNES sets, whose names differ in exactly two mechanical
/// ways: region is abbreviated, and dump-status flags are appended in square
/// brackets. `Super Mario World (U) [!]` and `Super Mario World (USA)` are the
/// same release under both conventions.
const REGION_ALIASES: [(&str, &str); 12] = [
    ("(U)", "(USA)"),
    ("(J)", "(Japan)"),
    ("(E)", "(Europe)"),
    ("(UE)", "(USA, Europe)"),
    ("(JU)", "(Japan, USA)"),
    ("(JE)", "(Japan, Europe)"),
    ("(F)", "(France)"),
    ("(G)", "(Germany)"),
    ("(S)", "(Spain)"),
    ("(I)", "(Italy)"),
    ("(A)", "(Australia)"),
    ("(B)", "(Brazil)"),
];

/// Drop `[...]` dump-status flags: `[!]` (verified good), `[b1]` (bad dump),
/// `[h1C]` (hack), and so on. They describe the dump, not the release, so they
/// never appear in a thumbnail name.
fn strip_dump_flags(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut depth = 0usize;
    for c in name.chars() {
        match c {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    // Collapse the double spaces left behind by a removed group.
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Names to try for a release, most-likely first.
///
/// The literal file name is always tried first, so a properly-named No-Intro set
/// costs exactly one request per game. Only when that misses is a GoodSNES-style
/// reading attempted, which is what lets an older set match at all -- verified
/// against this project's own `Super Mario World (U) [!].smc`, which the CDN
/// holds as `Super Mario World (USA).png`.
fn name_candidates(release: &str) -> Vec<String> {
    let mut candidates = vec![release.to_string()];

    let stripped = strip_dump_flags(release);
    if stripped != release && !stripped.is_empty() {
        candidates.push(stripped.clone());
    }

    // Region expansion, applied to whichever form survived flag-stripping.
    let base = if stripped.is_empty() { release } else { &stripped };
    for (short, long) in REGION_ALIASES {
        if base.ends_with(short) {
            let expanded = format!("{}{}", &base[..base.len() - short.len()], long);
            if !candidates.contains(&expanded) {
                candidates.push(expanded);
            }
            break;
        }
    }

    candidates
}

/// Cache key for a release. Derived from the name rather than from a hash of the
/// ROM so that no multi-megabyte read is needed to answer "do we already have
/// this?", and so two copies of the same release share one image.
fn cache_key(release: &str) -> String {
    // The name came from a real file name, so it is already legal here; only the
    // path separators need neutralising in case a caller passes something odd.
    release.replace(['/', '\\'], "_")
}

/// An existing cached image for this key, if any.
fn cached_image(dir: &Path, key: &str) -> Option<PathBuf> {
    IMAGE_EXTENSIONS.iter().find_map(|ext| {
        let candidate = dir.join(format!("{}.{}", key, ext));
        candidate.is_file().then_some(candidate)
    })
}

/// Marker recording that no art could be found for this key.
fn miss_marker(dir: &Path, key: &str) -> PathBuf {
    dir.join(format!("{}.miss", key))
}

/// Look for art the user already has, next to the ROM or in a sibling folder.
fn find_local_art(rom_path: &str, release: &str) -> Option<PathBuf> {
    let rom = Path::new(rom_path);
    let parent = rom.parent()?;

    for ext in IMAGE_EXTENSIONS {
        // Beside the ROM: "Super Mario World (USA).png"
        let beside = parent.join(format!("{}.{}", release, ext));
        if beside.is_file() {
            return Some(beside);
        }

        // In a conventional sibling folder.
        for folder in LOCAL_ART_DIRS {
            let nested = parent.join(folder).join(format!("{}.{}", release, ext));
            if nested.is_file() {
                return Some(nested);
            }
        }
    }

    None
}

/// Reject anything that is not actually an image.
///
/// A CDN or captive portal answering 200 with an HTML error page would otherwise
/// be written into the covers directory and then fail to render with no
/// explanation, which is far harder to diagnose than a clean miss.
fn looks_like_image(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else {
        None
    }
}

fn libretro_url(release: &str) -> String {
    format!(
        "{}/{}/{}/{}.png",
        LIBRETRO_BASE,
        encode_segment(LIBRETRO_SYSTEM),
        encode_segment(LIBRETRO_KIND),
        encode_segment(&libretro_name(release))
    )
}

/// Download a boxart, trying each naming convention in turn.
///
/// `Ok(None)` means no candidate matched (a real miss). `Err` means at least one
/// attempt could not be completed, and is returned in preference to a miss:
/// "we could not ask" must never be recorded as "there is no art".
fn download_libretro(release: &str) -> Result<Option<(Vec<u8>, &'static str)>, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(30))
        .user_agent(concat!("OxideSFC/", env!("CARGO_PKG_VERSION")))
        .build();

    let mut last_error = None;

    for candidate in name_candidates(release) {
        match download_one(&agent, &candidate) {
            Ok(Some(image)) => return Ok(Some(image)),
            Ok(None) => continue,
            Err(e) => last_error = Some(e),
        }
    }

    match last_error {
        Some(e) => Err(e),
        None => Ok(None),
    }
}

fn download_one(
    agent: &ureq::Agent,
    release: &str,
) -> Result<Option<(Vec<u8>, &'static str)>, String> {
    let url = libretro_url(release);
    debug!("Fetching cover: {}", url);

    match agent.get(&url).call() {
        Ok(response) => {
            let mut bytes = Vec::new();
            response
                .into_reader()
                .take(MAX_IMAGE_BYTES)
                .read_to_end(&mut bytes)
                .map_err(|e| format!("Failed to read cover response: {}", e))?;

            match looks_like_image(&bytes) {
                Some(ext) => Ok(Some((bytes, ext))),
                None => {
                    warn!("Cover response for {} was not an image", release);
                    Ok(None)
                }
            }
        }
        // 404 is the CDN's way of saying it has nothing under that name, which is
        // an answer, not a failure.
        Err(ureq::Error::Status(404, _)) => Ok(None),
        Err(ureq::Error::Status(code, _)) => {
            Err(format!("Cover host returned HTTP {}", code))
        }
        Err(e) => Err(format!("Cover request failed: {}", e)),
    }
}

fn finish(
    game_id: &str,
    path: PathBuf,
    source: CoverSource,
) -> Result<CoverResult, String> {
    let file = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| "Cover path has no file name".to_string())?;

    set_cover_file(game_id, Some(file.clone()))?;

    Ok(CoverResult {
        game_id: game_id.to_string(),
        path: Some(path.to_string_lossy().to_string()),
        file: Some(file),
        source,
    })
}

/// Resolve one game's cover.
///
/// Idempotent and safe to call repeatedly: an already-cached image short-circuits
/// immediately, and a recorded miss is not retried unless `force` is set. The
/// frontend drives concurrency by calling this per game, which keeps cancellation
/// trivial (stop queueing) and progress reporting exact.
///
/// `allow_download` off restricts the search to files already on disk, so the
/// local tier can be used with no network access at all.
#[tauri::command]
pub fn fetch_cover(
    game_id: String,
    allow_download: bool,
    force: bool,
) -> Result<CoverResult, String> {
    let game = get_game_by_id(&game_id)?
        .ok_or_else(|| format!("No game in the library with id {}", game_id))?;

    let dir = covers_dir()?;
    let release = release_name(&game.file_name);
    let key = cache_key(&release);

    // Already have it.
    if !force {
        if let Some(existing) = cached_image(&dir, &key) {
            // Re-record the association: the image can outlive the library entry
            // (a cleared and rescanned library keeps its covers).
            return finish(&game_id, existing, CoverSource::Cache);
        }
    }

    let marker = miss_marker(&dir, &key);
    if !force && marker.is_file() {
        return Ok(CoverResult {
            game_id,
            path: None,
            file: None,
            source: CoverSource::Missing,
        });
    }

    // Tier 1: local art.
    if let Some(local) = find_local_art(&game.file_path, &release) {
        let ext = local
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("png")
            .to_lowercase();
        let destination = dir.join(format!("{}.{}", key, ext));
        fs::copy(&local, &destination)
            .map_err(|e| format!("Failed to copy local cover: {}", e))?;
        info!("Cover for {} taken from {}", release, local.display());
        return finish(&game_id, destination, CoverSource::Local);
    }

    if !allow_download {
        return Ok(CoverResult {
            game_id,
            path: None,
            file: None,
            source: CoverSource::Unavailable,
        });
    }

    // Tier 2: Libretro CDN.
    match download_libretro(&release) {
        Ok(Some((bytes, ext))) => {
            let destination = dir.join(format!("{}.{}", key, ext));
            fs::write(&destination, &bytes)
                .map_err(|e| format!("Failed to write cover: {}", e))?;
            // A previous miss is now stale.
            let _ = fs::remove_file(&marker);
            info!("Downloaded cover for {}", release);
            finish(&game_id, destination, CoverSource::Libretro)
        }
        Ok(None) => {
            // Record the miss so this costs one request ever, not one per launch.
            if let Err(e) = fs::write(&marker, b"") {
                warn!("Failed to record cover miss for {}: {}", release, e);
            }
            Ok(CoverResult {
                game_id,
                path: None,
                file: None,
                source: CoverSource::Missing,
            })
        }
        Err(e) => {
            // Do not write a miss marker: this says nothing about whether art
            // exists, only that we could not ask.
            warn!("Cover lookup for {} could not complete: {}", release, e);
            Ok(CoverResult {
                game_id,
                path: None,
                file: None,
                source: CoverSource::Unavailable,
            })
        }
    }
}

/// Forget every cached cover and miss marker.
///
/// Exposed because a name-matched cache has no way to notice that the CDN gained
/// art for a release it previously lacked, so "try again from scratch" has to be
/// something the user can ask for.
#[tauri::command]
pub fn clear_cover_cache() -> Result<usize, String> {
    let dir = covers_dir()?;
    let mut removed = 0;

    for entry in fs::read_dir(&dir).map_err(|e| format!("Failed to read covers: {}", e))? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        let is_ours = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|ext| {
                let ext = ext.to_lowercase();
                ext == "miss" || IMAGE_EXTENSIONS.contains(&ext.as_str())
            })
            .unwrap_or(false);

        if is_ours && fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }

    set_cover_file_for_all(None)?;
    info!("Cleared {} cover file(s)", removed);
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_name_drops_the_extension_but_keeps_no_intro_decoration() {
        assert_eq!(
            release_name("Donkey Kong Country (USA) (Rev 1).sfc"),
            "Donkey Kong Country (USA) (Rev 1)"
        );
        // Archives are named after the release too, so they need no special case.
        assert_eq!(
            release_name("Legend of Zelda, The - A Link to the Past (USA).zip"),
            "Legend of Zelda, The - A Link to the Past (USA)"
        );
    }

    #[test]
    fn libretro_name_only_replaces_characters_illegal_in_file_names() {
        // Commas, parentheses and hyphens are pervasive in No-Intro names and
        // must survive untouched, or every lookup would miss.
        assert_eq!(
            libretro_name("Legend of Zelda, The - A Link to the Past (USA)"),
            "Legend of Zelda, The - A Link to the Past (USA)"
        );
        assert_eq!(
            libretro_name("Ren & Stimpy Show, The - Time Warp (USA)"),
            "Ren _ Stimpy Show, The - Time Warp (USA)"
        );
        assert_eq!(libretro_name("R/C Pro-Am"), "R_C Pro-Am");
    }

    #[test]
    fn segments_are_percent_encoded_so_spaces_and_brackets_survive() {
        assert_eq!(encode_segment("A Link to the Past"), "A%20Link%20to%20the%20Past");
        assert_eq!(encode_segment("Zelda, The (USA)"), "Zelda%2C%20The%20%28USA%29");
        assert_eq!(encode_segment("Super_Mario-World.png"), "Super_Mario-World.png");
    }

    #[test]
    fn dump_flags_are_stripped_but_release_parentheses_survive() {
        assert_eq!(strip_dump_flags("Super Mario World (U) [!]"), "Super Mario World (U)");
        assert_eq!(strip_dump_flags("Chrono Trigger (U) [b1][h1C]"), "Chrono Trigger (U)");
        assert_eq!(
            strip_dump_flags("Donkey Kong Country (USA) (Rev 1)"),
            "Donkey Kong Country (USA) (Rev 1)"
        );
    }

    #[test]
    fn goodsnes_names_fall_back_to_their_no_intro_spelling() {
        // The literal name must always be tried first, so a correctly-named
        // No-Intro set costs one request per game and nothing more.
        let candidates = name_candidates("Super Mario World (U) [!]");
        assert_eq!(candidates[0], "Super Mario World (U) [!]");
        assert!(
            candidates.contains(&"Super Mario World (USA)".to_string()),
            "expected a No-Intro candidate, got {:?}",
            candidates
        );

        assert_eq!(name_candidates("Secret of Mana (E)").last().unwrap(), "Secret of Mana (Europe)");
    }

    #[test]
    fn an_already_correct_name_produces_exactly_one_candidate() {
        // Otherwise every hit would still pay for speculative extra requests.
        assert_eq!(
            name_candidates("Donkey Kong Country (USA) (Rev 1)"),
            vec!["Donkey Kong Country (USA) (Rev 1)".to_string()]
        );
    }

    #[test]
    fn url_matches_the_cdn_layout() {
        let url = libretro_url("Donkey Kong Country (USA) (Rev 1)");
        assert!(url.starts_with("https://thumbnails.libretro.com/"));
        assert!(url.contains("Nintendo%20-%20Super%20Nintendo%20Entertainment%20System"));
        assert!(url.contains("Named_Boxarts"));
        assert!(url.ends_with("Donkey%20Kong%20Country%20%28USA%29%20%28Rev%201%29.png"));
    }

    #[test]
    fn image_sniffing_accepts_png_and_jpeg_and_rejects_html() {
        assert_eq!(
            looks_like_image(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00]),
            Some("png")
        );
        assert_eq!(looks_like_image(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("jpg"));
        assert_eq!(looks_like_image(b"<!DOCTYPE html><html>404"), None);
        assert_eq!(looks_like_image(b""), None);
    }
}
