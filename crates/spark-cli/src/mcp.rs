//! `spark mcp`: a Model Context Protocol server over stdio (newline-delimited JSON-RPC 2.0).
//!
//! Lets an AI agent drive the whole loop - create, check, test, look at the game, build - without a shell. Every tool
//! runs the matching `spark <command> --json` as a child process (same executable), so the MCP results are exactly what
//! the CLI prints and a crash in a game cannot take the server down.
//!
//! Scene and live-game tools (`scene_read`, `scene_apply`, `attach`, `game_control`, `detach`) run in this process:
//! see `mcp_scene.rs` and docs/dev/adr/0006-module-hot-reload-and-mcp-scene-tools.md.

use std::io::{BufRead, Write};

use serde_json::{Value, json};

use crate::util::*;

const PROTOCOL: &str = "2024-11-05";
const GUIDE: &str = include_str!("../../../docs/SPARK_API.md");
const REFERENCE: &str = include_str!("../../../docs/API_REFERENCE.md");
const SHADERS: &str = include_str!("../../../docs/SPARK_SHADERS.md");

fn path_prop() -> Value {
    json!({ "type": "string", "description": "Game folder (the one with main.luau)" })
}

fn tool(name: &str, description: &str, props: Value, required: &[&str]) -> Value {
    json!({ "name": name, "description": description, "inputSchema": { "type": "object", "properties": props, "required": required } })
}

pub fn tools() -> Vec<Value> {
    let mut all = base_tools();
    all.extend(crate::mcp_scene::tools());
    all
}

fn base_tools() -> Vec<Value> {
    vec![
        tool(
            "spark_new",
            "Create a new game folder (main.luau, src/, assets/, game.toml, AGENTS.md, docs/).",
            json!({ "path": path_prop(), "template": { "type": "string", "enum": ["3d", "2d", "empty"] } }),
            &["path"],
        ),
        tool(
            "spark_check",
            "Compile every .luau file and run the game headless for N frames. Returns {ok, errors:[{file,line,message}], notes}. Run after every edit.",
            json!({ "path": path_prop(), "frames": { "type": "integer", "minimum": 1, "maximum": 3600 }, "types": { "type": "boolean", "description": "also type-check with luau-lsp when installed" } }),
            &["path"],
        ),
        tool(
            "spark_test",
            "Run scenario tests (tests/*.luau) headless. Returns per-test pass/fail with file:line and the failing step.",
            json!({ "path": path_prop(), "filter": { "type": "string", "description": "only tests whose path contains this" }, "update": { "type": "boolean", "description": "accept changed snapshots (only for intended changes)" } }),
            &["path"],
        ),
        tool(
            "spark_screenshot",
            "Render the game headless and return the picture (PNG) plus its path. Look at it to judge visuals.",
            json!({ "path": path_prop(), "frames": { "type": "integer", "minimum": 1 }, "size": { "type": "string", "description": "WIDTHxHEIGHT, default 1280x720" } }),
            &["path"],
        ),
        tool(
            "spark_inspect",
            "Dump the scene after N frames: every object with name, position, rotation, scale, bounds.",
            json!({ "path": path_prop(), "frames": { "type": "integer", "minimum": 1 } }),
            &["path"],
        ),
        tool(
            "spark_golden",
            "Compare a headless render with tests/golden.png (tolerant). update=true writes the golden image.",
            json!({ "path": path_prop(), "update": { "type": "boolean" } }),
            &["path"],
        ),
        tool(
            "spark_doctor",
            "Check the environment (GPU, editor, luau-lsp) and, with path, the game's docs / version.",
            json!({ "path": path_prop() }),
            &[],
        ),
        tool(
            "spark_build",
            "Build a player executable (dist/<Name>) with everything packed inside.",
            json!({ "path": path_prop() }),
            &["path"],
        ),
        tool(
            "spark_docs_refresh",
            "Rewrite docs/, llms.txt and AGENTS.md of a game to this engine version.",
            json!({ "path": path_prop() }),
            &["path"],
        ),
        tool(
            "spark_api_search",
            "Find functions, tables, classes, fields in the Luau API by word (name or doc text).",
            json!({ "query": { "type": "string" }, "limit": { "type": "integer", "minimum": 1, "maximum": 50 } }),
            &["query"],
        ),
        tool(
            "spark_api_show",
            "Exact signature and docs of one API name, e.g. spawn, Mesh.cube, Object.rotate_x.",
            json!({ "name": { "type": "string" } }),
            &["name"],
        ),
        tool(
            "spark_guide",
            "Read the engine documentation: guide (concepts, units, frame order), reference (all signatures) or shaders.",
            json!({ "topic": { "type": "string", "enum": ["guide", "reference", "shaders"] } }),
            &["topic"],
        ),
    ]
}

fn str_arg<'a>(a: &'a Value, k: &str) -> Option<&'a str> {
    a.get(k).and_then(Value::as_str)
}

fn safe_path(a: &Value, required: bool) -> Result<Option<String>, String> {
    match str_arg(a, "path") {
        Some(p) if p.starts_with('-') => Err("path must not start with '-'".into()),
        Some("") => Err("path is empty".into()),
        Some(p) => Ok(Some(p.to_string())),
        None if required => Err("missing argument: path".into()),
        None => Ok(None),
    }
}

/// Result of one tool call: text, plus optional PNG bytes.
struct Out {
    text: String,
    image: Option<Vec<u8>>,
    error: bool,
}

fn text(s: String) -> Out {
    Out { text: s, image: None, error: false }
}

fn from_child(command: &[String]) -> Out {
    match run_self(command, None) {
        Ok(c) => {
            let body = if c.stdout.trim().is_empty() {
                json!({ "ok": c.code == 0, "exit_code": c.code, "stderr": tail(&c.stderr, 40) }).to_string()
            } else {
                c.stdout.trim().to_string()
            };
            Out { text: body, image: None, error: c.code != 0 }
        }
        Err(e) => Out { text: e, image: None, error: true },
    }
}

fn call(name: &str, a: &Value) -> Result<Out, String> {
    if let Some(r) = crate::mcp_scene::call(name, a) {
        return r.map(|(text, error)| Out { text, image: None, error });
    }
    let s = |v: &str| v.to_string();
    let path = |req| safe_path(a, req);
    Ok(match name {
        "spark_new" => {
            let p = path(true)?.unwrap_or_default();
            let mut args = vec![s("new"), p, s("--json")];
            if let Some(t) = str_arg(a, "template") {
                args.extend([s("--template"), t.to_string()]);
            }
            from_child(&args)
        }
        "spark_check" => {
            let mut args = vec![s("check"), path(true)?.unwrap_or_default(), s("--json")];
            if let Some(n) = a.get("frames").and_then(Value::as_u64) {
                args.extend([s("--frames"), n.clamp(1, 3600).to_string()]);
            }
            if a.get("types").and_then(Value::as_bool) == Some(true) {
                args.push(s("--types"));
            }
            from_child(&args)
        }
        "spark_test" => {
            let mut args = vec![s("test"), path(true)?.unwrap_or_default(), s("--json")];
            if let Some(f) = str_arg(a, "filter").filter(|f| !f.starts_with('-')) {
                args.push(f.to_string());
            }
            if a.get("update").and_then(Value::as_bool) == Some(true) {
                args.push(s("--update"));
            }
            from_child(&args)
        }
        "spark_screenshot" => {
            let out = std::env::temp_dir().join(format!("spark-mcp-shot-{}.png", std::process::id()));
            let mut args = vec![s("screenshot"), path(true)?.unwrap_or_default(), s("--json"), s("--out"), out.display().to_string()];
            if let Some(n) = a.get("frames").and_then(Value::as_u64) {
                args.extend([s("--frames"), n.to_string()]);
            }
            if let Some(sz) = str_arg(a, "size").filter(|z| z.chars().all(|c| c.is_ascii_digit() || c == 'x' || c == 'X')) {
                args.extend([s("--size"), sz.to_string()]);
            }
            let mut o = from_child(&args);
            if !o.error {
                o.image = std::fs::read(&out).ok();
                let _ = std::fs::remove_file(&out);
            }
            o
        }
        "spark_inspect" => {
            let mut args = vec![s("inspect"), path(true)?.unwrap_or_default(), s("--json")];
            if let Some(n) = a.get("frames").and_then(Value::as_u64) {
                args.extend([s("--frames"), n.to_string()]);
            }
            from_child(&args)
        }
        "spark_golden" => {
            let mut args = vec![s("golden"), path(true)?.unwrap_or_default(), s("--json")];
            if a.get("update").and_then(Value::as_bool) == Some(true) {
                args.push(s("--update"));
            }
            from_child(&args)
        }
        "spark_doctor" => {
            let mut args = vec![s("doctor"), s("--json")];
            args.extend(path(false)?);
            from_child(&args)
        }
        "spark_build" => from_child(&[s("build"), path(true)?.unwrap_or_default(), s("--json")]),
        "spark_docs_refresh" => from_child(&[s("docs"), path(true)?.unwrap_or_default(), s("--json")]),
        "spark_api_search" => {
            let q = str_arg(a, "query").ok_or("missing argument: query")?;
            let mut args = vec![s("api"), s("search")];
            args.extend(q.split_whitespace().filter(|w| !w.starts_with('-')).map(str::to_string));
            args.extend([s("--json"), s("--limit"), a.get("limit").and_then(Value::as_u64).unwrap_or(8).to_string()]);
            from_child(&args)
        }
        "spark_api_show" => {
            let n = str_arg(a, "name").filter(|n| !n.starts_with('-')).ok_or("missing argument: name")?;
            from_child(&[s("api"), s("show"), n.to_string(), s("--json")])
        }
        "spark_guide" => match str_arg(a, "topic") {
            Some("guide") => text(GUIDE.to_string()),
            Some("reference") => text(REFERENCE.to_string()),
            Some("shaders") => text(SHADERS.to_string()),
            _ => return Err("topic must be guide, reference or shaders".into()),
        },
        other => return Err(format!("unknown tool '{other}'")),
    })
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    out
}

fn rpc_error(id: Value, code: i32, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// One request -> at most one response (notifications have none).
pub fn handle(req: &Value) -> Option<Value> {
    let id = req.get("id").cloned();
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    let id = id?; // no id = a notification (e.g. notifications/initialized): no reply
    Some(match method {
        "initialize" => {
            let asked = req.pointer("/params/protocolVersion").and_then(Value::as_str).unwrap_or(PROTOCOL);
            json!({ "jsonrpc": "2.0", "id": id, "result": {
                "protocolVersion": asked,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "spark", "version": engine::VERSION },
                "instructions": "Spark game engine. Loop: spark_new -> edit main.luau / src/*.luau -> spark_check -> spark_test -> spark_screenshot (look at it) -> fix. Use spark_api_search / spark_guide instead of guessing names. Numbers must be finite; asset paths stay inside the game folder."
            } })
        }
        "ping" => json!({ "jsonrpc": "2.0", "id": id, "result": {} }),
        "tools/list" => json!({ "jsonrpc": "2.0", "id": id, "result": { "tools": tools() } }),
        "tools/call" => {
            let name = req.pointer("/params/name").and_then(Value::as_str).unwrap_or("");
            let args = req.pointer("/params/arguments").cloned().unwrap_or_else(|| json!({}));
            match call(name, &args) {
                Ok(o) => {
                    let mut content = vec![json!({ "type": "text", "text": o.text })];
                    if let Some(png) = o.image {
                        content.push(json!({ "type": "image", "data": base64(&png), "mimeType": "image/png" }));
                    }
                    json!({ "jsonrpc": "2.0", "id": id, "result": { "content": content, "isError": o.error } })
                }
                // Bad arguments are reported to the model as a tool error, so it can correct itself.
                Err(e) => json!({ "jsonrpc": "2.0", "id": id, "result": { "content": [{ "type": "text", "text": e }], "isError": true } }),
            }
        }
        _ => rpc_error(id, -32601, &format!("method not found: {method}")),
    })
}

pub fn run(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "usage: spark mcp\n\nModel Context Protocol server on stdin/stdout (JSON-RPC, one message per line).\nAdd it to an agent, e.g. Claude Code: claude mcp add spark -- spark mcp"
        );
        return EXIT_OK;
    }
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let resp = match serde_json::from_str::<Value>(&line) {
            Ok(Value::Array(batch)) => {
                let rs: Vec<Value> = batch.iter().filter_map(handle).collect();
                (!rs.is_empty()).then_some(Value::Array(rs))
            }
            Ok(req) => handle(&req),
            Err(e) => Some(rpc_error(Value::Null, -32700, &format!("parse error: {e}"))),
        };
        if let Some(r) = resp {
            if writeln!(out, "{r}").and_then(|_| out.flush()).is_err() {
                break;
            }
        }
    }
    EXIT_OK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_spec() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn handshake_and_tools() {
        let r = handle(&json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}})).unwrap();
        assert_eq!(r["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(r["result"]["serverInfo"]["name"], "spark");
        assert!(handle(&json!({"jsonrpc":"2.0","method":"notifications/initialized"})).is_none());
        let r = handle(&json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})).unwrap();
        let names: Vec<&str> = r["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(
            names.contains(&"spark_check")
                && names.contains(&"spark_screenshot")
                && names.contains(&"scene_apply")
                && names.contains(&"attach")
        );
        let r = handle(&json!({"jsonrpc":"2.0","id":3,"method":"nope"})).unwrap();
        assert_eq!(r["error"]["code"], -32601);
    }

    #[test]
    fn tool_arguments_are_validated() {
        let r =
            handle(&json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"spark_check","arguments":{"path":"--evil"}}}))
                .unwrap();
        assert_eq!(r["result"]["isError"], true);
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("must not start"));
        let r = handle(&json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"nope","arguments":{}}})).unwrap();
        assert_eq!(r["result"]["isError"], true);
        let r = handle(
            &json!({"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"spark_guide","arguments":{"topic":"reference"}}}),
        )
        .unwrap();
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("Spark Luau API reference"));
    }
}
