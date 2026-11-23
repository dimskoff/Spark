//! `spark migrate [--check] [--json] <path>...` - bring `.scene` / `.prefab` files to the current format.
//!
//! A path is a file, or a folder searched recursively (folders starting with `.`, `target` and `node_modules` are
//! skipped). Files that are already current are never touched. With `--check` nothing is written and the exit code is
//! `1` when a file needs migration (or cannot be read), so it can run in CI. Exit codes as everywhere: `0` ok, `1` a file
//! needs migration / is broken, `2` bad usage.

use std::path::{Path, PathBuf};

use serde_json::json;

use crate::util::*;

const USAGE: &str = "usage: spark migrate [--check] [--json] <path>...   (a .scene / .prefab file, or a folder with them)";

/// What happened to one file.
#[derive(Debug, PartialEq)]
enum Status {
    Current { version: u32 },
    NeedsMigration { from: u32, to: u32 },
    Migrated { from: u32, to: u32 },
    Error(String),
}

impl Status {
    fn code(&self) -> &'static str {
        match self {
            Status::Current { .. } => "current",
            Status::NeedsMigration { .. } => "needs_migration",
            Status::Migrated { .. } => "migrated",
            Status::Error(_) => "error",
        }
    }

    fn describe(&self) -> String {
        match self {
            Status::Current { version } => format!("up to date (format_version {version})"),
            Status::NeedsMigration { from, to } => format!("needs migration (format_version {from} -> {to})"),
            Status::Migrated { from, to } => format!("migrated (format_version {from} -> {to})"),
            Status::Error(e) => format!("error: {e}"),
        }
    }
}

pub fn run(mut args: Vec<String>) -> i32 {
    let json = take_flag(&mut args, "--json");
    let check = take_flag(&mut args, "--check");
    if let Some(flag) = args.iter().find(|a| a.starts_with("--")) {
        return usage(json, &format!("unknown flag '{flag}'"));
    }
    if args.is_empty() {
        return usage(json, "missing path");
    }
    let mut files = Vec::new();
    for a in &args {
        let p = Path::new(a);
        if p.is_dir() {
            collect(p, &mut files);
        } else if p.is_file() {
            files.push(p.to_path_buf());
        } else {
            return usage(json, &format!("'{a}' does not exist"));
        }
    }
    files.sort();
    files.dedup();

    let results: Vec<(PathBuf, Status)> = files
        .into_iter()
        .map(|f| {
            let s = process(&f, check);
            (f, s)
        })
        .collect();
    let failed = results.iter().any(|(_, s)| match s {
        Status::Error(_) => true,
        Status::NeedsMigration { .. } => check,
        _ => false,
    });
    let count = |code: &str| results.iter().filter(|(_, s)| s.code() == code).count();
    if json {
        let files: Vec<_> = results
            .iter()
            .map(|(p, s)| {
                let mut v = json!({ "path": plain_path(p), "status": s.code() });
                if let Some(o) = v.as_object_mut() {
                    match s {
                        Status::Current { version } => {
                            o.insert("version".into(), json!(version));
                        }
                        Status::NeedsMigration { from, to } | Status::Migrated { from, to } => {
                            o.insert("from".into(), json!(from));
                            o.insert("to".into(), json!(to));
                        }
                        Status::Error(e) => {
                            o.insert("error".into(), json!(e));
                        }
                    }
                }
                v
            })
            .collect();
        print_json(&json!({
            "command": "migrate",
            "ok": !failed,
            "exit_code": if failed { EXIT_FAILED } else { EXIT_OK },
            "check": check,
            "current_version": spark_scene::FORMAT_VERSION,
            "files": files,
            "summary": {
                "current": count("current"),
                "migrated": count("migrated"),
                "needs_migration": count("needs_migration"),
                "errors": count("error"),
            },
        }));
    } else {
        for (p, s) in &results {
            println!("{}: {}", p.display(), s.describe());
        }
        if results.is_empty() {
            println!("no .scene or .prefab files found");
        } else if check && count("needs_migration") > 0 {
            println!("run `spark migrate` without --check to update them");
        }
    }
    if failed { EXIT_FAILED } else { EXIT_OK }
}

fn process(path: &Path, check: bool) -> Status {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => return Status::Error(format!("cannot read the file: {e}")),
    };
    let version = match spark_scene::read_version(&text) {
        Ok(v) => v,
        Err(e) => return Status::Error(e.to_string()),
    };
    let to = spark_scene::FORMAT_VERSION;
    if version == to {
        return Status::Current { version };
    }
    if check {
        return Status::NeedsMigration { from: version, to };
    }
    match spark_scene::migrate_text(&text) {
        Ok(m) => match std::fs::write(path, m.text) {
            Ok(()) => Status::Migrated { from: m.from, to: m.to },
            Err(e) => Status::Error(format!("cannot write the file: {e}")),
        },
        Err(e) => Status::Error(e.to_string()),
    }
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if p.is_dir() {
            if !name.starts_with('.') && name != "target" && name != "node_modules" {
                collect(&p, out);
            }
        } else if matches!(p.extension().and_then(|x| x.to_str()), Some("scene" | "prefab")) {
            out.push(p);
        }
    }
}

fn usage(json: bool, msg: &str) -> i32 {
    if json {
        print_json(&json_error("migrate", msg));
    } else {
        eprintln!("spark migrate: {msg}\n{USAGE}");
    }
    EXIT_USAGE
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("spark-migrate-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    const V0: &str = "[scene]\nformat_version = 0\n\n[[node]]\nid = \"a\"\n\n[node.light]\nkind = { kind = \"spot\" }\n";
    const V1: &str = "[scene]\nformat_version = 1\n\n[[node]]\nid = \"a\"\n";

    #[test]
    fn check_reports_and_does_not_write() {
        let d = temp_dir("check");
        std::fs::write(d.join("old.scene"), V0).unwrap();
        std::fs::write(d.join("new.scene"), V1).unwrap();
        assert_eq!(run(vec!["--check".into(), d.display().to_string()]), EXIT_FAILED);
        assert_eq!(std::fs::read_to_string(d.join("old.scene")).unwrap(), V0, "--check never writes");
        assert_eq!(process(&d.join("old.scene"), true), Status::NeedsMigration { from: 0, to: 1 });
        assert_eq!(process(&d.join("new.scene"), true), Status::Current { version: 1 });
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn migrate_rewrites_old_files_only() {
        let d = temp_dir("write");
        std::fs::create_dir_all(d.join("levels")).unwrap();
        std::fs::create_dir_all(d.join(".hidden")).unwrap();
        std::fs::write(d.join("levels").join("old.scene"), V0).unwrap();
        std::fs::write(d.join(".hidden").join("skipped.scene"), V0).unwrap();
        let messy = "[scene]\nformat_version=1\n";
        std::fs::write(d.join("current.scene"), messy).unwrap();
        assert_eq!(run(vec![d.display().to_string()]), EXIT_OK);
        let migrated = std::fs::read_to_string(d.join("levels").join("old.scene")).unwrap();
        assert!(migrated.contains("format_version = 1") && migrated.contains("type = \"spot\""), "{migrated}");
        assert_eq!(std::fs::read_to_string(d.join("current.scene")).unwrap(), messy, "a current file is not rewritten");
        assert_eq!(std::fs::read_to_string(d.join(".hidden").join("skipped.scene")).unwrap(), V0);
        // Now everything is current: --check passes.
        assert_eq!(run(vec!["--check".into(), d.display().to_string()]), EXIT_OK);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn broken_and_future_files_are_errors() {
        let d = temp_dir("broken");
        std::fs::write(d.join("future.scene"), "[scene]\nformat_version = 99\n").unwrap();
        std::fs::write(d.join("bad.scene"), "[scene\n").unwrap();
        let Status::Error(m) = process(&d.join("future.scene"), false) else { panic!("future files are errors") };
        assert!(m.contains("update Spark"), "{m}");
        assert!(matches!(process(&d.join("bad.scene"), false), Status::Error(_)));
        assert_eq!(run(vec![d.display().to_string()]), EXIT_FAILED);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn usage_errors() {
        assert_eq!(run(vec![]), EXIT_USAGE);
        assert_eq!(run(vec!["--bogus".into(), "x".into()]), EXIT_USAGE);
        assert_eq!(run(vec!["definitely/not/here.scene".into()]), EXIT_USAGE);
    }
}
