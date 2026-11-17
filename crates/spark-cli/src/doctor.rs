//! `spark doctor [path/to/game] [--json]`: is this machine ready to build and run Spark games?

use std::path::Path;

use serde_json::{Value, json};

use crate::util::*;

fn check(name: &str, status: &str, detail: impl Into<String>) -> Value {
    json!({ "name": name, "status": status, "detail": detail.into() })
}

/// A program on `PATH` (runs `--version`).
fn on_path(program: &str) -> Option<String> {
    let out = std::process::Command::new(program).arg("--version").stdin(std::process::Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).lines().next().unwrap_or("").trim().to_string())
}

pub fn run(mut args: Vec<String>) -> i32 {
    let json = take_flag(&mut args, "--json");
    let mut checks = vec![check("spark", "ok", format!("{} on {} {}", engine::VERSION, std::env::consts::OS, std::env::consts::ARCH))];

    match std::env::current_exe() {
        Ok(exe) => {
            let editor = exe.with_file_name(if cfg!(windows) { "spark-editor.exe" } else { "spark-editor" });
            if editor.is_file() {
                checks.push(check("editor", "ok", plain_path(&editor)));
            } else {
                checks.push(check(
                    "editor",
                    "warn",
                    "spark-editor is not next to spark (`spark editor` will not work; the release archives contain it)",
                ));
            }
        }
        Err(e) => checks.push(check("executable", "fail", format!("cannot locate this executable: {e}"))),
    }

    match engine::Renderer::new_headless(16, 16) {
        Ok(r) => {
            let info = r.info();
            let software = info.device_type.to_lowercase().contains("cpu");
            checks.push(check(
                "gpu",
                if software { "warn" } else { "ok" },
                format!(
                    "{info}{}",
                    if software { " - software rendering: screenshots work, but are slow and may differ from a real GPU" } else { "" }
                ),
            ));
        }
        Err(e) => checks.push(check(
            "gpu",
            "fail",
            format!("no usable graphics adapter: {e} (headless `spark check` and `spark test` still work; screenshots and windows do not)"),
        )),
    }

    checks.push(match on_path("luau-lsp") {
        Some(v) => check("luau-lsp", "ok", v),
        None => check(
            "luau-lsp",
            "warn",
            "not installed: `spark check --types` is skipped (optional; install from github.com/JohnnyMorganz/luau-lsp)",
        ),
    });

    let tmp = std::env::temp_dir().join(format!("spark-doctor-{}", std::process::id()));
    checks.push(match std::fs::create_dir_all(&tmp).and_then(|_| std::fs::write(tmp.join("t"), b"x")) {
        Ok(()) => {
            let _ = std::fs::remove_dir_all(&tmp);
            check("temp folder", "ok", plain_path(&std::env::temp_dir()))
        }
        Err(e) => check("temp folder", "fail", format!("cannot write to {}: {e}", std::env::temp_dir().display())),
    });

    if let Some(game) = args.first() {
        let dir = Path::new(game);
        let dir = if dir.is_file() { dir.parent().unwrap_or(Path::new(".")) } else { dir };
        if !dir.join("main.luau").is_file() {
            checks.push(check("game", "fail", format!("'{}' has no main.luau (create one with `spark new`)", dir.display())));
        } else {
            match spark_project::manifest::Manifest::load(dir) {
                Ok(m) => {
                    let detail =
                        m.version_warning(engine::VERSION).map(|w| format!("game.toml: {w}")).unwrap_or_else(|| "game.toml ok".into());
                    let status = if detail.contains("game.toml:") { "warn" } else { "ok" };
                    checks.push(check("game", status, detail));
                }
                Err(e) => checks.push(check("game", "fail", e)),
            }
            let docs_ok = dir.join("docs/spark.d.luau").is_file()
                && std::fs::read_to_string(dir.join("docs/spark.d.luau")).map(|t| t == spark_api::D_LUAU).unwrap_or(false);
            checks.push(if docs_ok {
                check("game docs", "ok", "docs/ match this engine version")
            } else {
                check("game docs", "warn", "docs/ are missing or from another engine version: run `spark docs .`")
            });
        }
    }

    let failed = checks.iter().any(|c| c["status"] == "fail");
    if json {
        print_json(
            &json!({ "command": "doctor", "ok": !failed, "exit_code": if failed { EXIT_USAGE } else { EXIT_OK }, "checks": checks }),
        );
    } else {
        for c in &checks {
            let mark = match c["status"].as_str() {
                Some("ok") => "ok  ",
                Some("warn") => "warn",
                _ => "FAIL",
            };
            println!("[{mark}] {}: {}", c["name"].as_str().unwrap_or(""), c["detail"].as_str().unwrap_or(""));
        }
    }
    if failed { EXIT_USAGE } else { EXIT_OK }
}
