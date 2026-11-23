//! Shared helpers for the machine-readable side of the CLI.
//!
//! Contract of every command (documented in `docs/SPARK_API.md`, "Command line"):
//! exit code `0` = ok, `1` = the game, a test or a check failed, `2` = bad usage or a broken environment.
//! With `--json` the command prints exactly one JSON object on stdout and nothing else.

use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

pub const EXIT_OK: i32 = 0;
pub const EXIT_FAILED: i32 = 1;
pub const EXIT_USAGE: i32 = 2;

/// Removes `flag` from `args`; true if it was there.
pub fn take_flag(args: &mut Vec<String>, flag: &str) -> bool {
    let before = args.len();
    args.retain(|a| a != flag);
    args.len() != before
}

/// Value of `--name VALUE`, removed from `args`. `Err` when the value is missing.
pub fn take_value(args: &mut Vec<String>, name: &str) -> Result<Option<String>, String> {
    let Some(i) = args.iter().position(|a| a == name) else { return Ok(None) };
    if i + 1 >= args.len() {
        return Err(format!("{name} needs a value"));
    }
    let v = args.remove(i + 1);
    args.remove(i);
    Ok(Some(v))
}

/// Prints one JSON object (the only thing on stdout in `--json` mode).
pub fn print_json(v: &Value) {
    println!("{}", serde_json::to_string(v).unwrap_or_else(|_| "{}".into()));
}

/// `{"ok":false,"error":..}` for usage / environment errors in `--json` mode.
pub fn json_error(command: &str, message: &str) -> Value {
    json!({ "command": command, "ok": false, "exit_code": EXIT_USAGE, "error": message })
}

/// Result of running this executable again with other arguments.
pub struct Child {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Runs `spark <args>` as a child process (same executable) and captures its output. Used to give commands
/// that print human text a `--json` envelope, and by the MCP server.
pub fn run_self(args: &[String], cwd: Option<&Path>) -> Result<Child, String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot find the spark executable: {e}"))?;
    let mut cmd = Command::new(exe);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(d) = cwd {
        cmd.current_dir(d);
    }
    let out = cmd.output().map_err(|e| format!("cannot start spark: {e}"))?;
    Ok(Child {
        code: out.status.code().unwrap_or(EXIT_FAILED),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}

/// Last `n` lines (log output of a child can be long; agents need the end).
pub fn tail(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// Strips the `\\?\` prefix Windows adds to canonical paths (agents and humans copy paths).
pub fn plain_path(p: &Path) -> String {
    let s = p.display().to_string();
    s.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(s)
}

/// Runs `command args` as a child and wraps its human output in a JSON envelope:
/// `{"command","ok","exit_code","stdout","stderr", ...extra}`.
pub fn envelope(command: &str, args: &[String], extra: Value) -> i32 {
    let mut all = vec![command.to_string()];
    all.extend(args.iter().cloned());
    match run_self(&all, None) {
        Ok(c) => {
            let mut v = json!({
                "command": command,
                "ok": c.code == 0,
                "exit_code": c.code,
                "stdout": c.stdout.trim_end(),
                "stderr": tail(&c.stderr, 40),
            });
            if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
                o.extend(e.clone());
            }
            print_json(&v);
            c.code
        }
        Err(e) => {
            print_json(&json_error(command, &e));
            EXIT_USAGE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags() {
        let mut a: Vec<String> = ["x", "--json", "--out", "p.png", "y"].iter().map(|s| s.to_string()).collect();
        assert!(take_flag(&mut a, "--json"));
        assert!(!take_flag(&mut a, "--json"));
        assert_eq!(take_value(&mut a, "--out").unwrap().as_deref(), Some("p.png"));
        assert_eq!(a, ["x", "y"]);
        let mut b: Vec<String> = vec!["--out".into()];
        assert!(take_value(&mut b, "--out").is_err());
        assert_eq!(tail("a\nb\nc", 2), "b\nc");
    }
}
