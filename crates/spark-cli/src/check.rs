//! `spark check path/to/game [--frames N] [--json]`: finds script errors without a window or GPU.
//!
//! 1. Compiles every `.luau` file of the game (syntax errors in modules that are not required yet).
//! 2. Runs `main.luau` headless (physics on, no audio, no renderer) for N frames (default 60):
//!    `start()`, `update`, `fixed_update`, `render`, timers, tasks and every `require`d module.
//!
//! Prints `file:line: message` per error (or a JSON report with `--json`); exit code 1 on any error.

use std::path::{Path, PathBuf};

use engine::script::ScriptGame;
use engine::{Game, World, run_frame};

use crate::manifest::Manifest;

const USAGE: &str = "usage: spark check path/to/game [--frames N] [--json] [--types]";
/// Folders whose `.luau` files are not part of the game.
const SKIP_DIRS: &[&str] = &["dist", "target", "node_modules", "screenshots"];

#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    pub file: String,
    pub line: Option<u32>,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct Report {
    pub errors: Vec<Problem>,
    pub frames: u32,
    pub objects: usize,
    /// `.luau` files compiled.
    pub files: usize,
    /// Things that did not fail the check but the reader should know (e.g. `--types` without luau-lsp).
    pub notes: Vec<String>,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn to_json(&self) -> String {
        let errors: Vec<String> = self
            .errors
            .iter()
            .map(|e| {
                let line = e.line.map(|l| l.to_string()).unwrap_or_else(|| "null".into());
                format!("{{\"file\":{},\"line\":{line},\"message\":{}}}", json_str(&e.file), json_str(&e.message))
            })
            .collect();
        let notes: Vec<String> = self.notes.iter().map(|n| json_str(n)).collect();
        format!(
            "{{\"ok\":{},\"errors\":[{}],\"frames\":{},\"objects\":{},\"files\":{},\"notes\":[{}]}}",
            self.ok(),
            errors.join(","),
            self.frames,
            self.objects,
            self.files,
            notes.join(",")
        )
    }
}

pub fn run(args: &[String]) -> i32 {
    let mut game: Option<PathBuf> = None;
    let mut frames = 60u32;
    let mut json = false;
    let mut types = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => json = true,
            "--types" => types = true,
            "--frames" => match it.next().and_then(|n| n.parse().ok()) {
                Some(n) => frames = n,
                None => {
                    eprintln!("spark check: --frames needs a number, e.g. --frames 120\n{USAGE}");
                    return 2;
                }
            },
            "--help" | "-h" => {
                println!("{USAGE}");
                return 0;
            }
            _ if game.is_none() && !a.starts_with("--") => game = Some(PathBuf::from(a)),
            _ => {
                eprintln!("spark check: unexpected argument '{a}'\n{USAGE}");
                return 2;
            }
        }
    }
    let game = game.unwrap_or_else(|| PathBuf::from("."));
    let mut report = check(&game, frames);
    if types {
        check_types(&game, &mut report);
    }
    if json {
        println!("{}", report.to_json());
    } else {
        for e in &report.errors {
            match e.line {
                Some(l) => eprintln!("{}:{l}: {}", e.file, e.message),
                None => eprintln!("{}: {}", e.file, e.message),
            }
        }
        for n in &report.notes {
            eprintln!("spark check: note: {n}");
        }
        if report.ok() {
            println!("spark check: ok ({} files, {} frames, {} objects)", report.files, report.frames, report.objects);
        } else {
            eprintln!("spark check: {} error(s)", report.errors.len());
        }
    }
    if report.ok() { 0 } else { 1 }
}

/// Checks the game at `path` (folder or its main.luau).
pub fn check(path: &Path, frames: u32) -> Report {
    let mut report = Report::default();
    let main = if path.is_dir() { path.join("main.luau") } else { path.to_path_buf() };
    let Some(dir) = main.parent().map(|d| if d.as_os_str().is_empty() { Path::new(".") } else { d }).and_then(|d| d.canonicalize().ok()) else {
        report.errors.push(problem(&main.display().to_string(), None, "game folder not found"));
        return report;
    };
    if !main.is_file() {
        let msg = format!("no main.luau in '{}' (create a game with: spark new {})", dir.display(), path.display());
        report.errors.push(problem("main.luau", None, &msg));
        return report;
    }
    let main = dir.join(main.file_name().unwrap_or_default());
    if let Err(e) = Manifest::load(&dir) {
        report.errors.push(problem("game.toml", None, &e));
    }

    // 1. Syntax of every module, required or not.
    let mut files = Vec::new();
    luau_files(&dir, &dir, &mut files);
    report.files = files.len();
    let lua = mlua::Lua::new();
    for rel in &files {
        let name = rel.to_string_lossy().replace('\\', "/");
        let source = match std::fs::read_to_string(dir.join(rel)) {
            Ok(s) => s,
            Err(e) => {
                report.errors.push(problem(&name, None, &format!("cannot read: {e}")));
                continue;
            }
        };
        if let Err(e) = lua.load(source.as_str()).set_name(format!("@{name}")).into_function() {
            report.errors.push(parse_error(&e.to_string(), &name));
        }
    }
    // 1b. Assets, whether a script loads them in the first N frames or not: WGSL shaders compile and validate,
    // images and glTF models decode. (The GPU itself is not used, so pipeline-level failures still need a render.)
    check_assets(&dir, &mut report);
    if !report.ok() {
        // Running would only repeat the static errors.
        return report;
    }

    // 2. Run headless.
    let error_log = dir.join("error.log");
    let had_error_log = error_log.exists();
    let mut game = ScriptGame::new(main.clone()).hot_reload(false);
    let mut world = World::new();
    world.physics.set_backend(Box::new(engine::physics::RapierBackend::new()));
    game.start(&mut world);
    while report.frames < frames && game.error().is_none() && !world.quit_requested() {
        run_frame(&mut game, &mut world, 1.0 / 60.0);
        report.frames += 1;
    }
    report.objects = world.scene.len();
    if let Some(e) = game.error() {
        let main_name = main.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        report.errors.push(parse_error(e, &main_name));
    }
    if !had_error_log {
        let _ = std::fs::remove_file(&error_log);
    }
    report
}

/// Files below `dir` with one of `exts` (lowercase, no dot), relative to `root`, sorted, without hidden / build folders.
fn files_with_ext(root: &Path, dir: &Path, exts: &[&str], out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            if !SKIP_DIRS.contains(&name.as_str()) {
                files_with_ext(root, &path, exts, out);
            }
        } else if path.extension().is_some_and(|e| exts.contains(&e.to_string_lossy().to_lowercase().as_str())) {
            out.push(path.strip_prefix(root).unwrap_or(&path).to_path_buf());
        }
    }
}

/// Static checks of the game's assets: every `.wgsl` compiles and validates (the same check `Shader.load` runs),
/// every `.png` / `.jpg` decodes, every `.gltf` / `.glb` parses.
fn check_assets(dir: &Path, report: &mut Report) {
    let mut files = Vec::new();
    files_with_ext(dir, dir, &["wgsl", "png", "jpg", "jpeg", "gltf", "glb"], &mut files);
    for rel in files {
        let name = rel.to_string_lossy().replace('\\', "/");
        let full = dir.join(&rel);
        let ext = rel.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let result = match ext.as_str() {
            "wgsl" => std::fs::read_to_string(&full)
                .map_err(|e| format!("cannot read: {e}"))
                .and_then(|code| engine::shader::ShaderData::compile(&name, &code).map(|_| ())),
            "png" | "jpg" | "jpeg" => engine::ImageData::load(&full).map(|_| ()),
            _ => engine::model::ModelData::load(&full, &mut engine::Assets::new()).map(|_| ()),
        };
        if let Err(e) = result {
            report.errors.push(problem(&name, None, &e));
        }
    }
}

/// `--types`: type-checks the game with luau-lsp against `docs/spark.d.luau` (wrong API names, arguments and
/// field types that only fail at runtime otherwise). luau-lsp is optional: without it a note says how to get it.
fn check_types(path: &Path, report: &mut Report) {
    let dir = if path.is_dir() { path.to_path_buf() } else { path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(".")) };
    let defs = dir.join("docs/spark.d.luau");
    if !defs.is_file() {
        report.notes.push("--types: docs/spark.d.luau is missing (run `spark docs .` first)".into());
        return;
    }
    let mut files = Vec::new();
    luau_files(&dir, &dir, &mut files);
    let files: Vec<String> = files.iter().map(|f| f.to_string_lossy().replace('\\', "/")).collect();
    let output = std::process::Command::new("luau-lsp")
        .current_dir(&dir)
        .args(["analyze", "--definitions=@spark=docs/spark.d.luau", "--formatter=gcc"])
        .args(&files)
        .output();
    let output = match output {
        Ok(o) => o,
        Err(_) => {
            report.notes.push(
                "--types needs luau-lsp (https://github.com/JohnnyMorganz/luau-lsp/releases) on PATH; skipped the type check".into(),
            );
            return;
        }
    };
    let text = format!("{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    let mut found = parse_lsp_output(&text);
    if found.is_empty() && !output.status.success() {
        let tail: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).take(3).collect();
        report.notes.push(format!("luau-lsp failed without a diagnostic: {}", tail.join(" | ")));
    }
    report.errors.append(&mut found);
}

/// Errors from `luau-lsp analyze` output: `file.luau:12:5: error: message` (gcc) or `file.luau(12,5): TypeError: message`.
/// Warnings and hints are ignored.
pub fn parse_lsp_output(text: &str) -> Vec<Problem> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        let Some(at) = line.find(".luau") else { continue };
        let lower = line.to_lowercase();
        if !(lower.contains("error:") || lower.contains("typeerror") || lower.contains("syntaxerror")) {
            continue;
        }
        let file_end = at + 5;
        let rest = &line[file_end..];
        let Some(open) = rest.chars().next().filter(|c| matches!(c, ':' | '(')) else { continue };
        let digits: String = rest[open.len_utf8()..].chars().take_while(|c| c.is_ascii_digit()).collect();
        let Ok(number) = digits.parse::<u32>() else { continue };
        let file = line[..file_end].replace('\\', "/");
        // The message is whatever follows the last "error:" / "TypeError:" marker.
        let message = ["TypeError:", "SyntaxError:", "error:"]
            .iter()
            .filter_map(|m| line.find(m).map(|i| line[i + m.len()..].trim()))
            .next()
            .unwrap_or(line);
        out.push(Problem { file, line: Some(number), message: message.to_string() });
    }
    out
}

fn problem(file: &str, line: Option<u32>, message: &str) -> Problem {
    Problem { file: file.into(), line, message: message.trim().into() }
}

/// Splits `src/a.luau:12: message` (anywhere in a Luau error / traceback) into file, line, message.
/// The first `<file>.luau:<line>:` location wins; without one the error is attributed to `fallback`.
pub fn parse_error(text: &str, fallback: &str) -> Problem {
    let text = text.trim();
    let mut search = 0;
    while let Some(pos) = text[search..].find(".luau") {
        let at = search + pos;
        // `file.luau:12:` or `[string "file.luau"]:12:`
        let after = text[at + 5..].trim_start_matches(['"', '\'', ']']);
        let Some(after) = after.strip_prefix(':') else {
            search = at + 5;
            continue;
        };
        let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            let start = text[..at]
                .char_indices()
                .rev()
                .find(|(_, c)| c.is_whitespace() || matches!(c, '"' | '\'' | '(' | '[' | '@' | '<'))
                .map(|(i, c)| i + c.len_utf8())
                .unwrap_or(0);
            let file = text[start..at + 5].replace('\\', "/");
            let line = digits.parse().ok();
            let rest = after[digits.len()..].trim_start_matches(':').trim();
            // Keep any context in front of the location ("in start(): ...") and the rest (stack trace, hint).
            let prefix = text[..start].trim().trim_end_matches(['[', '"', '\'', '(', '@', '<']).trim();
            let message = if prefix.is_empty() || prefix.ends_with("error:") { rest.to_string() } else { format!("{prefix} {rest}") };
            return Problem { file, line, message: message.trim().into() };
        }
        search = at + 5;
    }
    problem(fallback, None, text)
}

/// `.luau` files below `dir` (relative to `root`), sorted, without hidden / build folders.
fn luau_files(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            if !SKIP_DIRS.contains(&name.as_str()) {
                luau_files(root, &path, out);
            }
        } else if name.ends_with(".luau") && !name.ends_with(".d.luau") {
            out.push(path.strip_prefix(root).unwrap_or(&path).to_path_buf());
        }
    }
}

fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(files: &[(&str, &str)]) -> PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("spark-check-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&dir);
        for (rel, text) in files {
            let p = dir.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        dir
    }

    #[test]
    fn parses_locations() {
        let p = parse_error("src/player.luau:12: attempt to index nil with 'x'\nstack traceback:\n...", "main.luau");
        assert_eq!((p.file.as_str(), p.line), ("src/player.luau", Some(12)));
        assert!(p.message.starts_with("attempt to index nil"), "{}", p.message);
        let p = parse_error("runtime error: [string \"main.luau\"]:3: boom", "main.luau");
        assert_eq!((p.file.as_str(), p.line), ("main.luau", Some(3)));
        let p = parse_error("something without a location", "main.luau");
        assert_eq!((p.file.as_str(), p.line), ("main.luau", None));
        let p = parse_error(r"C:\games\x\src\a.luau:7: bad", "main.luau");
        assert_eq!((p.file.as_str(), p.line), ("C:/games/x/src/a.luau", Some(7)));
    }

    #[test]
    fn ok_game_runs_frames() {
        let dir = game(&[
            ("main.luau", "local M = require(\"./src/m\")\nfunction start() M.build() end\nfunction update(dt) local _ = input.text() .. tostring(input.repeated(\"minus\")) end\n"),
            ("src/m.luau", "local M = {}\nfunction M.build() spawn(Mesh.cube(), \"red\") end\nreturn M\n"),
        ]);
        let r = check(&dir, 10);
        assert!(r.ok(), "{:?}", r.errors);
        assert_eq!((r.frames, r.objects, r.files), (10, 1, 2));
        assert!(r.to_json().starts_with("{\"ok\":true,\"errors\":[],\"frames\":10,\"objects\":1"), "{}", r.to_json());
        assert!(!dir.join("error.log").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn finds_syntax_error_in_unrequired_module() {
        let dir = game(&[("main.luau", "function start() end\n"), ("src/broken.luau", "local x = \n\nlocal = 3\n")]);
        let r = check(&dir, 10);
        assert_eq!(r.errors.len(), 1, "{:?}", r.errors);
        assert_eq!(r.errors[0].file, "src/broken.luau");
        assert!(r.errors[0].line.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn finds_runtime_error_in_module() {
        let dir = game(&[
            ("main.luau", "local M = require(\"./src/m\")\nfunction update(dt)\n    if time.frame > 3 then M.fail() end\nend\n"),
            ("src/m.luau", "local M = {}\nfunction M.fail()\n    input.down(\"nokey\")\nend\nreturn M\n"),
        ]);
        let r = check(&dir, 60);
        assert_eq!(r.errors.len(), 1, "{:?}", r.errors);
        let e = &r.errors[0];
        assert_eq!((e.file.as_str(), e.line), ("src/m.luau", Some(3)), "{e:?}");
        assert!(e.message.contains("unknown key 'nokey'"), "{}", e.message);
        assert!(r.frames < 60);
        let json = r.to_json();
        assert!(json.starts_with("{\"ok\":false,\"errors\":[{\"file\":\"src/m.luau\",\"line\":3,"), "{json}");
        assert!(!dir.join("error.log").exists(), "check leaves no error.log behind");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn text_input_reaches_scripts() {
        let dir = game(&[("main.luau", "typed = \"\"\nfunction update(dt) typed ..= input.text() if typed == \"hi\" then quit() end end\n")]);
        let mut game = ScriptGame::new(dir.join("main.luau")).hot_reload(false);
        let mut world = World::new();
        game.start(&mut world);
        world.input.text_event("hi\r");
        run_frame(&mut game, &mut world, 1.0 / 60.0);
        assert!(game.error().is_none(), "{:?}", game.error());
        assert!(world.quit_requested());
        assert_eq!(world.input.text, "", "cleared after the frame");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn finds_broken_assets_nobody_loads() {
        let dir = game(&[
            ("main.luau", "function start() end\n"),
            ("assets/shaders/good.wgsl", "fn fragment(f: Fragment) -> vec4<f32> { return vec4<f32>(1.0, 0.0, 0.0, 1.0); }\n"),
            ("assets/shaders/bad.wgsl", "fn fragment(f: Fragment) -> vec4<f32> { return nope; }\n"),
            ("assets/textures/broken.png", "this is not a png"),
        ]);
        let r = check(&dir, 5);
        let files: Vec<_> = r.errors.iter().map(|e| e.file.as_str()).collect();
        assert_eq!(files, ["assets/shaders/bad.wgsl", "assets/textures/broken.png"], "{:?}", r.errors);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_luau_lsp_output() {
        let text = "src/player.luau:12:5: error: Type 'string' could not be converted into 'number'\n\
                    src/hud.luau:3:1: warning: unused local\n\
                    main.luau(7,2): TypeError: Unknown global 'spwan'\n\
                    some unrelated line\n";
        let p = parse_lsp_output(text);
        assert_eq!(p.len(), 2, "{p:?}");
        assert_eq!((p[0].file.as_str(), p[0].line), ("src/player.luau", Some(12)));
        assert!(p[0].message.starts_with("Type 'string'"), "{}", p[0].message);
        assert_eq!((p[1].file.as_str(), p[1].line), ("main.luau", Some(7)));
        assert_eq!(p[1].message, "Unknown global 'spwan'");
    }

    #[test]
    fn types_without_luau_lsp_is_a_note_not_a_failure() {
        let dir = game(&[("main.luau", "function start() end\n")]);
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::write(dir.join("docs/spark.d.luau"), "").unwrap();
        let mut r = check(&dir, 3);
        check_types(&dir, &mut r);
        // luau-lsp is usually absent on CI; either way the check must not invent errors for a clean game.
        assert!(r.ok() || r.errors.iter().all(|e| e.line.is_some()), "{:?}", r.errors);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_main() {
        let dir = game(&[("readme.txt", "x")]);
        let r = check(&dir, 1);
        assert!(!r.ok() && r.errors[0].message.contains("no main.luau"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
