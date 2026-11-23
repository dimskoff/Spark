//! Per-user folders of a game (saves, logs) and best-effort log writing.
//!
//! Layout: `<user data dir>/SparkGames/<game>/` with `save.json` and `logs/`.
//! The user data dir is `%APPDATA%` (Windows), `~/Library/Application Support` (macOS) or
//! `$XDG_DATA_HOME` / `~/.local/share` (Linux).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Folder name of a game: unsafe characters become `_`, never empty.
pub fn clean_name(game: &str) -> String {
    let clean: String = game.chars().map(|c| if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.') { c } else { '_' }).collect();
    let clean = clean.trim().trim_matches('.').to_string();
    if clean.is_empty() { "game".to_string() } else { clean }
}

/// `<base>/SparkGames/<game>` (pure: `base` is the user data dir).
pub fn game_dir_in(base: &Path, game: &str) -> PathBuf {
    base.join("SparkGames").join(clean_name(game))
}

/// `<base>/SparkGames/<game>/logs` (pure).
pub fn log_dir_in(base: &Path, game: &str) -> PathBuf {
    game_dir_in(base, game).join("logs")
}

/// The per-user data folder of this platform (`None` when the environment does not say).
pub fn user_data_base() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| Path::new(&h).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".local/share")))
    }
}

/// `<user data dir>/SparkGames/<game>`.
pub fn game_dir(game: &str) -> Option<PathBuf> {
    user_data_base().map(|b| game_dir_in(&b, game))
}

/// Where logs of `game` go when the game folder is not writable or not on disk (packed games):
/// `<user data dir>/SparkGames/<game>/logs`.
pub fn log_dir(game: &str) -> Option<PathBuf> {
    user_data_base().map(|b| log_dir_in(&b, game))
}

/// Folders to try for a log file, best first. Developers (and AI agents) look next to the game, so a dev run
/// writes there; a packed game has no real folder, so it only uses `logs` (the per-user logs dir).
/// A failed write to the first one falls back to the next.
pub fn log_candidates(dev_dir: Option<&Path>, packed: bool, logs: Option<&Path>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if !packed {
        out.extend(dev_dir.map(Path::to_path_buf));
    }
    out.extend(logs.map(Path::to_path_buf));
    out
}

/// Writes `<dir>/<file>` for the first folder in `dirs` that works (folders are created). Returns the path written.
pub fn write_log(dirs: &[PathBuf], file: &str, text: &str) -> Option<PathBuf> {
    dirs.iter().find_map(|d| {
        std::fs::create_dir_all(d).ok()?;
        let p = d.join(file);
        std::fs::write(&p, text).ok()?;
        Some(p)
    })
}

static GAME_NAME: Mutex<Option<String>> = Mutex::new(None);

/// Remembers the game's name (the name used for its save / logs folder) for crash reports.
pub fn set_game_name(name: &str) {
    if let Ok(mut g) = GAME_NAME.lock() {
        *g = Some(name.to_string());
    }
}

/// The name given to [`set_game_name`].
pub fn game_name() -> Option<String> {
    GAME_NAME.lock().ok().and_then(|g| g.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_cleaned() {
        assert_eq!(clean_name("My Game: 2/3"), "My Game_ 2_3");
        assert_eq!(clean_name("..."), "game");
        assert_eq!(clean_name(""), "game");
    }

    #[test]
    fn log_dir_is_under_the_game_dir() {
        let base = Path::new("base");
        assert_eq!(log_dir_in(base, "Cube Run"), base.join("SparkGames").join("Cube Run").join("logs"));
        assert_eq!(game_dir_in(base, "a/b"), base.join("SparkGames").join("a_b"));
    }

    #[test]
    fn dev_runs_write_next_to_the_game_packed_runs_to_the_logs_dir() {
        let (dev, logs) = (Path::new("game"), Path::new("logs"));
        assert_eq!(log_candidates(Some(dev), false, Some(logs)), vec![dev.to_path_buf(), logs.to_path_buf()]);
        assert_eq!(log_candidates(Some(dev), true, Some(logs)), vec![logs.to_path_buf()]);
        assert!(log_candidates(Some(dev), true, None).is_empty());
        assert_eq!(log_candidates(None, false, None), Vec::<PathBuf>::new());
    }

    #[test]
    fn write_log_creates_dirs_and_falls_back() {
        let root = std::env::temp_dir().join(format!("spark-paths-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        // A file where the first folder should be: creating it fails, the second one is used.
        let blocked = root.join("blocked");
        std::fs::write(&blocked, "x").unwrap();
        let logs = root.join("a").join("logs");
        let written = write_log(&[blocked.join("sub"), logs.clone()], "error.log", "boom\n").unwrap();
        assert_eq!(written, logs.join("error.log"));
        assert_eq!(std::fs::read_to_string(written).unwrap(), "boom\n");
        assert!(write_log(&[blocked.join("sub")], "error.log", "x").is_none());
        let _ = std::fs::remove_dir_all(&root);
    }
}
