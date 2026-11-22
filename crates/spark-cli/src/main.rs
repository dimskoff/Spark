//! `spark path/to/game [flags]` - runs a Luau game (a folder means `folder/main.luau`).
//! Without a path: `main.luau` in the current folder, else next to the executable (shipped games:
//! put the exe, `main.luau` and the assets in one folder and double-click the exe).
//!
//! `spark editor [path/to/game]` - opens Spark Editor (welcome screen, or the game when it is a Spark project).
//! `spark new path/to/game` - a new game with the recommended folder layout (+ AGENTS.md and the API docs).
//! `spark docs path/to/game` - refresh the API docs inside a game after an engine update.
//! `spark update [version]` - replace this spark executable with a GitHub release (Windows, Linux, macOS).
//! `spark check path/to/game [--frames N] [--json]` - find script errors headless, without a window or GPU.
//! `spark test path/to/game [NAME...] [--update] [--json]` - scenario tests in `tests/` (see `test.rs`).
//!
//! `spark build path/to/game` - a player build: ONE executable (this binary, no console window) with
//! every game file packed inside, plus a .zip. When such an executable starts it runs its packed game.
#![cfg_attr(all(feature = "gui", windows), windows_subsystem = "windows")]

mod api_cmd;
mod check;
mod dist;
mod doctor;
mod editor;
mod icon;
mod mcp;
mod new;
mod test;
mod update;
mod util;
mod visual;

use spark_project::manifest;

use std::path::{Path, PathBuf};

use engine::script::{ScriptGame, default_save_path};
use engine::{App, HELP, ImageData};

use manifest::Manifest;

/// Third-party license notices (one copy, embedded by the `spark` crate). Printed by `spark licenses` and written next to every `spark build` output.
pub use engine::THIRD_PARTY_LICENSES;

const USAGE: &str = "Usage:
  spark path/to/game [flags]              run a game (folder with main.luau, or the .luau file)
  spark editor [path/to/game]             open Spark Editor (the game, or the welcome screen)
  spark new path/to/game                  create a new game with the recommended layout
    [--template 3d|2d|empty]              3d (default): small 3D scene, 2d: draw.* game, empty: only the skeleton
  spark docs path/to/game                 write the API docs + AGENTS.md (for AI agents) into a game
  spark check path/to/game [--frames N]   find script errors without a window or GPU (default 60 frames)
                           [--json]       machine-readable report: {ok, errors:[{file,line,message}], frames, objects, files, notes}
                           [--types]      also type-check with luau-lsp + docs/spark.d.luau (when luau-lsp is installed)
  spark test path/to/game [NAME...]       run scenario tests (tests/*.luau) headless; snapshots in tests/snapshots/
                          [--update]      accept changed snapshots   [--json]   [--max-time SECONDS]
  spark api search <words>|show <name>|list   look the Luau API up [--json]
  spark doctor [path/to/game]             check this machine (GPU, editor, luau-lsp) and the game's docs [--json]
  spark screenshot path/to/game           render a PNG headless [--frames N] [--size WxH] [--out FILE] [--json]
  spark inspect path/to/game              dump the scene (objects, positions, bounds) [--frames N] [--json]
  spark golden path/to/game               compare a render with tests/golden.png [--update] [--tolerance T] [--json]
  spark diff a.png b.png                  compare two images [--tolerance T] [--out diff.png] [--json]
  spark mcp                               Model Context Protocol server on stdio (for AI agents)
  spark licenses                          print the third-party license notices (also shipped as THIRD_PARTY_LICENSES.txt with every build)
  spark update [version]                  download the latest (or given) Spark release from GitHub
  spark --version
  spark build path/to/game [--out DIR]    make a player build: one executable with everything inside (+ .zip / .tar.gz)
                           [--loose]      exe + game files in a folder instead
                           [--no-zip]
                           [--app]        macOS: also wrap it in a <Name>.app bundle";

fn main() {
    // A player build: the game is packed inside this executable.
    if let Some(pack) = engine::vfs::Pack::embedded() {
        run_packed(pack);
        return;
    }
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let rest: Vec<String> = raw.iter().skip(1).cloned().collect();
    match raw.first().map(String::as_str) {
        Some("--help" | "-h" | "help") => {
            println!("{USAGE}\n\n{HELP}");
            return;
        }
        Some(cmd @ ("build" | "new" | "docs" | "update")) if rest.iter().any(|a| a == "--json") => {
            // These print human text; with --json the text is wrapped in one JSON object.
            let mut args = rest.clone();
            util::take_flag(&mut args, "--json");
            let path = args.iter().find(|a| !a.starts_with("--")).cloned();
            std::process::exit(util::envelope(cmd, &args, serde_json::json!({ "path": path })));
        }
        Some("build") => std::process::exit(dist::run(&raw[1..])),
        Some("editor") => std::process::exit(editor::run(&raw[1..])),
        Some("new") => std::process::exit(new::run(&raw[1..])),
        Some("docs") => std::process::exit(new::run_docs(&raw[1..])),
        Some("check") => std::process::exit(check::run(&raw[1..])),
        Some("test") => std::process::exit(test::run(&raw[1..])),
        Some("update") => std::process::exit(update::run(&raw[1..])),
        Some("api") => std::process::exit(api_cmd::run(rest)),
        Some("doctor") => std::process::exit(doctor::run(rest)),
        Some("licenses") => {
            print!("{THIRD_PARTY_LICENSES}");
            return;
        }
        Some("mcp") => std::process::exit(mcp::run(rest)),
        Some("screenshot") => std::process::exit(visual::screenshot(rest)),
        Some("inspect") => std::process::exit(visual::inspect(rest)),
        Some("diff") => std::process::exit(visual::diff(rest)),
        Some("golden") => std::process::exit(visual::golden(rest)),
        Some("--version" | "-V") => {
            println!("spark {}", engine::VERSION);
            return;
        }
        _ => {}
    }
    let app = App::new("Spark");
    let arg = app.args().rest.first().cloned();
    let mut path = match arg {
        Some(a) => PathBuf::from(a),
        None => {
            let here = PathBuf::from("main.luau");
            let beside_exe = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("main.luau")));
            match beside_exe {
                Some(p) if !here.is_file() && p.is_file() => p,
                _ => here,
            }
        }
    };
    if path.is_dir() {
        path = path.join("main.luau");
    }
    if !path.is_file() {
        eprintln!("spark: script not found: {}\n{USAGE}\n\n{HELP}", path.display());
        std::process::exit(2);
    }
    let dir = path.canonicalize().ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_default();
    let path = dir.join(path.file_name().unwrap_or_default());
    run(app, path, dir, false);
}

/// Runs the game packed in this executable (no hot reload: the files can't change).
fn run_packed(pack: engine::vfs::Pack) {
    let exe = std::env::current_exe().ok();
    let dir = exe.as_ref().and_then(|e| e.parent()).map(Path::to_path_buf).unwrap_or_default();
    let dir = dir.canonicalize().unwrap_or(dir);
    if !pack.contains("main.luau") {
        eprintln!("spark: the game pack in this executable has no main.luau (rebuild it with spark build)");
        std::process::exit(2);
    }
    engine::vfs::mount(&dir, pack);
    let app = App::new("Spark");
    run(app, dir.join("main.luau"), dir, true);
}

fn run(app: App, path: PathBuf, dir: PathBuf, packed: bool) {
    let manifest = match Manifest::load(&dir) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("spark: {e}");
            std::process::exit(2);
        }
    };
    if !packed {
        if let Some(w) = manifest.version_warning(engine::VERSION) {
            eprintln!("spark: warning: {w}");
        }
    }
    let folder = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Spark".into());
    let title = manifest.title.clone().unwrap_or(folder);
    let headless = app.args().headless;
    let mut app = app.title(title.clone()).size(manifest.width, manifest.height).vsync(manifest.vsync);
    app = app.fullscreen(manifest.fullscreen).splash(manifest.splash).fps_in_title(manifest.show_fps.unwrap_or(manifest.title.is_none()));
    if let Some(icon) = &manifest.icon {
        match ImageData::load(&dir.join(icon)) {
            Ok(img) => app = app.icon(img),
            Err(e) => eprintln!("spark: game.toml icon: {e}"),
        }
    }
    let game_name = manifest.save_name.as_deref().unwrap_or(&title).to_string();
    let save = if headless { None } else { default_save_path(&game_name) };
    // Crash reports (`spark-crash.log`) and packed games' `error.log` go to <user data>/SparkGames/<game>/logs.
    engine::paths::set_game_name(&game_name);
    let seed = app.args().effective_seed();
    app.run(
        ScriptGame::new(path)
            .strict(headless)
            .hot_reload(!headless && !packed)
            .save_path(save)
            .seed(seed)
            .log_dir(engine::paths::log_dir(&game_name)),
    );
}
