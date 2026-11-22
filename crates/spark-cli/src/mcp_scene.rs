//! MCP tools that read and edit scenes and drive a live game (ADR 0006): `scene_read`, `scene_apply`, `attach`,
//! `game_control`, `detach`.
//!
//! * Offline (`path` is a `.scene` file): load it into a headless `World`, apply the commands through `World::apply`,
//!   save the world back (the file is only written when every command applied).
//! * Live (no `path`, a game attached): the same commands go over devtools protocol 4 to the running game.
//!
//! Commands are the JSON form of `spark_devtools::command_json`. A rejected command answers
//! `{"ok":false,"error":{"code","message","path","suggestions"}}`; an accepted one answers `inverse`, a single
//! command that undoes the whole call (send it back as the only entry of `commands`).

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::{Child, Command as Proc, Stdio};
use std::time::{Duration, Instant};

use engine::{Command, World};
use serde_json::{Value, json};
use spark_devtools::command_json::{command_from_json, command_to_json, widen_ints};
use spark_devtools::{CommandErrorInfo, EditorCommand, GameHello, GameLink, GameMessage, Server, ServerEvent, Stats};
use spark_scene::{FsLoader, load_scene, save_scene};

pub fn tools() -> Vec<Value> {
    let tool = |name: &str, description: &str, props: Value, required: &[&str]| json!({ "name": name, "description": description, "inputSchema": { "type": "object", "properties": props, "required": required } });
    vec![
        tool(
            "scene_read",
            "Read a .scene file (path given: its text and node count) or, with no path, the live scene of the attached game as a \
             snapshot {frame, camera, objects:[{id,node_id,name,parent,position,...}]}.",
            json!({ "path": { "type": "string", "description": ".scene file; omit to read the attached game" } }),
            &[],
        ),
        tool(
            "scene_apply",
            "Apply commands to a .scene file (path given; saved only if all apply) or to the attached game (no path). A command is \
             {\"op\":\"set\",\"node\":ID,\"path\":\"transform.position\",\"value\":[0,1,0]} or spawn, delete, reparent, add_component, \
             remove_component, batch. All-or-nothing. Success: {ok, changed, inverse, resolved}; send `inverse` back as the only \
             command to undo. Failure: {ok:false, error:{code,message,path,suggestions}}.",
            json!({
                "path": { "type": "string", "description": ".scene file; omit to edit the attached game" },
                "root": { "type": "string", "description": "game folder for prefabs and assets (default: the scene's folder)" },
                "commands": { "type": "array", "items": { "type": "object" }, "minItems": 1 },
                "dry_run": { "type": "boolean", "description": "offline only: check and return the inverse, do not write the file" }
            }),
            &["commands"],
        ),
        tool(
            "attach",
            "Start a game window with devtools and connect to it (path = game folder), or with listen=true only open the \
             authenticated loopback port and return {addr, token}: start the game yourself with SPARK_DEVTOOLS_TOKEN=<token> \
             spark <game> --devtools <addr>. One game at a time.",
            json!({
                "path": { "type": "string", "description": "game folder (with main.luau) to launch" },
                "listen": { "type": "boolean" },
                "timeout_ms": { "type": "integer", "minimum": 100, "maximum": 120000 }
            }),
            &[],
        ),
        tool(
            "game_control",
            "Drive the attached game: status, pause, resume, step (frames, while paused), reload (restart the scripts), overlay \
             (on), stats.",
            json!({
                "action": { "type": "string", "enum": ["status", "pause", "resume", "step", "reload", "overlay", "stats"] },
                "frames": { "type": "integer", "minimum": 1 },
                "on": { "type": "boolean" }
            }),
            &["action"],
        ),
        tool("detach", "Close the attached game (asks it to quit, then stops a launched process) and forget the session.", json!({}), &[]),
    ]
}

/// Result of a tool: the JSON text and whether it is an error.
pub type ToolOut = (String, bool);

pub fn call(name: &str, a: &Value) -> Option<Result<ToolOut, String>> {
    let r = match name {
        "scene_read" => scene_read(a),
        "scene_apply" => scene_apply(a),
        "attach" => attach(a),
        "game_control" => game_control(a),
        "detach" => detach(),
        _ => return None,
    };
    Some(r.map(|v| {
        let bad = v.get("ok") == Some(&Value::Bool(false));
        (v.to_string(), bad)
    }))
}

fn error_json(e: &CommandErrorInfo) -> Value {
    json!({ "ok": false, "error": { "code": e.code, "message": e.message, "path": e.path, "suggestions": e.suggestions } })
}

fn fail(code: &str, message: impl Into<String>) -> Value {
    json!({ "ok": false, "error": { "code": code, "message": message.into(), "path": null, "suggestions": [] } })
}

fn path_arg(a: &Value) -> Result<Option<PathBuf>, String> {
    match a.get("path").and_then(Value::as_str) {
        None => Ok(None),
        Some(p) if p.is_empty() || p.starts_with('-') => Err("path is empty or starts with '-'".into()),
        Some(p) => Ok(Some(PathBuf::from(p))),
    }
}

// ---------------------------------------------------------------------------------------------------------
// Inverses that have no JSON form (exact states) are kept here and handed out as `{"op":"restore","token":N}`.

const MAX_TOKENS: usize = 256;

#[derive(Default)]
struct Tokens {
    next: u64,
    stored: VecDeque<(u64, Command)>,
}

thread_local! {
    static TOKENS: RefCell<Tokens> = RefCell::new(Tokens::default());
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
}

fn encode_inverse(cmd: &Command) -> Value {
    match cmd {
        Command::Batch(v) => json!({ "op": "batch", "commands": v.iter().map(encode_inverse).collect::<Vec<_>>() }),
        other => command_to_json(other).unwrap_or_else(|_| {
            TOKENS.with(|t| {
                let mut t = t.borrow_mut();
                let token = t.next;
                t.next += 1;
                t.stored.push_back((token, other.clone()));
                if t.stored.len() > MAX_TOKENS {
                    t.stored.pop_front();
                }
                json!({ "op": "restore", "token": token })
            })
        }),
    }
}

/// `commands` as one `batch` command JSON.
fn batch_json(a: &Value) -> Result<Value, Value> {
    match a.get("commands") {
        Some(Value::Array(list)) if !list.is_empty() => Ok(json!({ "op": "batch", "commands": list })),
        _ => Err(fail("bad_command", "`commands` must be a non-empty array of command objects")),
    }
}

/// Applies the batch to a world: all or nothing.
fn apply_batch(world: &mut World, batch: &Value) -> Result<Value, Value> {
    let resolve = |t: u64| TOKENS.with(|s| s.borrow().stored.iter().find(|(k, _)| *k == t).map(|(_, c)| c.clone()));
    let cmd = command_from_json(batch, &resolve).map_err(|e| error_json(&CommandErrorInfo::from_json_error(&e)))?;
    let mut result = world.apply(&cmd);
    if matches!(&result, Err(e) if e.code() == "wrong_type") {
        // JSON has one number type: `5` is an integer and a float field rejects it. Retry with floats.
        if let Some(Ok(a)) = widen_ints(&cmd).map(|w| world.apply(&w)) {
            result = Ok(a);
        }
    }
    let applied = result.map_err(|e| error_json(&CommandErrorInfo::from_command_error(&e)))?;
    Ok(json!({
        "ok": true,
        "changed": applied.changed.iter().map(|n| n.to_string()).collect::<Vec<_>>(),
        "inverse": encode_inverse(&applied.inverse),
        "resolved": applied.resolved.as_ref().and_then(|c| command_to_json(c).ok()),
    }))
}

// ---------------------------------------------------------------------------------------------------------
// Offline

fn read_scene(path: &Path) -> Result<(String, spark_scene::Loaded), Value> {
    let text = std::fs::read_to_string(path).map_err(|e| fail("cannot_read", format!("{}: {e}", path.display())))?;
    match spark_scene::parse(&text) {
        Ok(l) => Ok((text, l)),
        Err(e) => Err(fail(e.code(), format!("{}: {e}", path.display()))),
    }
}

fn scene_root(a: &Value, scene: &Path) -> PathBuf {
    match a.get("root").and_then(Value::as_str) {
        Some(r) if !r.is_empty() && !r.starts_with('-') => PathBuf::from(r),
        _ => scene.parent().map(Path::to_path_buf).filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| PathBuf::from(".")),
    }
}

fn scene_read(a: &Value) -> Result<Value, String> {
    if let Some(path) = path_arg(a)? {
        return Ok(match read_scene(&path) {
            Ok((text, loaded)) => {
                json!({ "ok": true, "mode": "file", "path": path.display().to_string(), "nodes": loaded.file.nodes.len(), "text": text })
            }
            Err(e) => e,
        });
    }
    live_read()
}

fn scene_apply(a: &Value) -> Result<Value, String> {
    let batch = match batch_json(a) {
        Ok(b) => b,
        Err(e) => return Ok(e),
    };
    let Some(path) = path_arg(a)? else { return live_apply(&batch) };
    let root = scene_root(a, &path);
    let (_, loaded) = match read_scene(&path) {
        Ok(r) => r,
        Err(e) => return Ok(e),
    };
    let mut world = World::new();
    world.assets.root = root.clone();
    world.assets.confine = true;
    let mut loader = FsLoader::new(&root, true);
    if let Err(e) = load_scene(&mut world, &loaded.file, &mut loader, true) {
        return Ok(fail("load_failed", format!("{}: {e}", path.display())));
    }
    let mut out = match apply_batch(&mut world, &batch) {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };
    let saved = match save_scene(&mut world, &mut loader) {
        Ok(s) => s,
        Err(e) => return Ok(fail("save_failed", e.to_string())),
    };
    let dry = a.get("dry_run").and_then(Value::as_bool) == Some(true);
    if !dry {
        let tmp = path.with_extension("scene.spark-tmp");
        if let Err(e) = std::fs::write(&tmp, &saved.text).and_then(|_| std::fs::rename(&tmp, &path)) {
            let _ = std::fs::remove_file(&tmp);
            return Ok(fail("cannot_write", format!("{}: {e}", path.display())));
        }
    }
    out["mode"] = "file".into();
    out["path"] = path.display().to_string().into();
    out["written"] = (!dry).into();
    out["warnings"] = json!(saved.warnings);
    Ok(out)
}

// ---------------------------------------------------------------------------------------------------------
// Live

struct Session {
    server: Option<Server>,
    link: Option<GameLink>,
    child: Option<Child>,
    token: String,
    addr: String,
    hello: Option<GameHello>,
    stats: Option<Stats>,
    paused: bool,
    script_error: Option<String>,
    next_request: u64,
}

impl Session {
    /// Accepts the game when it connects and takes in what it sent. Returns the messages for the caller to look at.
    fn pump(&mut self) -> Vec<GameMessage> {
        if let Some(server) = &mut self.server {
            for ev in server.poll_game() {
                match ev {
                    ServerEvent::Connected(link, hello) => {
                        self.link = Some(link);
                        self.hello = Some(hello);
                    }
                    ServerEvent::Rejected(m) | ServerEvent::GaveUp(m) => eprintln!("spark mcp: {m}"),
                }
            }
            if self.link.is_some() {
                self.server = None;
            }
        }
        let msgs: Vec<GameMessage> = self.link.as_ref().map(|l| l.drain().collect()).unwrap_or_default();
        for m in &msgs {
            match m {
                GameMessage::Stats(s) => self.stats = Some(s.clone()),
                GameMessage::Paused { paused } => self.paused = *paused,
                GameMessage::ScriptError { message, node_id: None } => self.script_error = message.clone(),
                _ => {}
            }
        }
        msgs
    }

    fn connected(&self) -> bool {
        self.link.as_ref().is_some_and(|l| l.is_alive())
    }

    /// Pumps until `f` accepts a message or `timeout` passes.
    fn wait<T>(&mut self, timeout: Duration, mut f: impl FnMut(&GameMessage) -> Option<T>) -> Option<T> {
        let start = Instant::now();
        loop {
            for m in self.pump() {
                if let Some(t) = f(&m) {
                    return Some(t);
                }
            }
            if start.elapsed() >= timeout || (self.link.is_some() && !self.connected()) {
                return None;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn describe(&self) -> Value {
        json!({
            "connected": self.connected(),
            "waiting_for_game": self.link.is_none(),
            "addr": self.addr,
            "protocol": self.hello.as_ref().map(|h| h.protocol),
            "engine": self.hello.as_ref().map(|h| h.engine.clone()),
            "pid": self.hello.as_ref().map(|h| h.pid),
            "gpu": self.hello.as_ref().map(|h| h.gpu.clone()),
            "paused": self.paused,
            "script_error": self.script_error,
            "stats": self.stats,
        })
    }
}

fn with_live<T>(f: impl FnOnce(&mut Session) -> Result<T, Value>) -> Result<T, Value> {
    SESSION.with(|s| {
        let mut s = s.borrow_mut();
        let Some(session) = s.as_mut() else {
            return Err(fail("not_attached", "no game is attached: call `attach` first (or pass `path` to work on a .scene file)"));
        };
        session.pump();
        if !session.connected() {
            return Err(fail("not_connected", "the game has not connected (yet) or has exited; call `detach`, then `attach`"));
        }
        f(session)
    })
}

fn live_read() -> Result<Value, String> {
    Ok(with_live(|s| {
        let link = s.link.as_ref().expect("connected");
        link.send(EditorCommand::WatchScene { interval_ms: Some(16) });
        let scene = s.wait(Duration::from_secs(5), |m| if let GameMessage::Scene(sc) = m { Some(sc.clone()) } else { None });
        if let Some(l) = &s.link {
            l.send(EditorCommand::WatchScene { interval_ms: None });
        }
        match scene {
            Some(sc) => Ok(json!({ "ok": true, "mode": "live", "scene": sc })),
            None => Err(fail("timeout", "the game did not send its scene within 5 s")),
        }
    })
    .unwrap_or_else(|e| e))
}

fn live_apply(batch: &Value) -> Result<Value, String> {
    Ok(with_live(|s| {
        if s.hello.as_ref().is_some_and(|h| h.protocol < 4) {
            return Err(fail("protocol_too_old", "the game speaks devtools protocol < 4 and cannot take commands"));
        }
        s.next_request += 1;
        let request = s.next_request;
        s.link.as_ref().expect("connected").send(EditorCommand::Command { request, json: batch.clone() });
        let result = s.wait(Duration::from_secs(5), |m| match m {
            GameMessage::CommandResult(r) if r.request == request => Some(r.clone()),
            _ => None,
        });
        let Some(r) = result else { return Err(fail("timeout", "the game did not answer within 5 s")) };
        if let Some(e) = r.error {
            return Err(error_json(&e));
        }
        Ok(json!({ "ok": true, "mode": "live", "changed": r.changed, "inverse": r.inverse, "resolved": r.resolved }))
    })
    .unwrap_or_else(|e| e))
}

fn attach(a: &Value) -> Result<Value, String> {
    if SESSION.with(|s| s.borrow().is_some()) {
        return Ok(fail("already_attached", "a game is already attached: call `detach` first"));
    }
    let listen = a.get("listen").and_then(Value::as_bool) == Some(true);
    let game = path_arg(a)?;
    if game.is_none() && !listen {
        return Err("give `path` (the game folder to launch) or `listen: true`".into());
    }
    let timeout = Duration::from_millis(a.get("timeout_ms").and_then(Value::as_u64).unwrap_or(30_000).clamp(100, 120_000));
    let server = Server::bind_local_authenticated().map_err(|e| format!("devtools: {e}"))?;
    let token = server.token().unwrap_or_default().to_string();
    let addr = server.addr().to_string();
    let mut child = None;
    if let (Some(game), false) = (&game, listen) {
        let exe = std::env::current_exe().map_err(|e| format!("cannot find the spark executable: {e}"))?;
        // The token goes through the environment (argv is visible to every process).
        let c = Proc::new(exe)
            .arg(game)
            .arg(spark_devtools::FLAG)
            .arg(&addr)
            .env(spark_devtools::TOKEN_ENV, &token)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("cannot start the game: {e}"))?;
        child = Some(c);
    }
    let mut session = Session {
        server: Some(server),
        link: None,
        child,
        token: token.clone(),
        addr: addr.clone(),
        hello: None,
        stats: None,
        paused: false,
        script_error: None,
        next_request: 0,
    };
    if listen {
        let out = json!({ "ok": true, "state": "waiting", "addr": addr, "token": token,
            "hint": "start the game with SPARK_DEVTOOLS_TOKEN=<token> spark <game> --devtools <addr>; other tools work once it connected" });
        SESSION.with(|s| *s.borrow_mut() = Some(session));
        return Ok(out);
    }
    let start = Instant::now();
    loop {
        session.pump();
        if session.link.is_some() {
            break;
        }
        let exited = session.child.as_mut().and_then(|c| c.try_wait().ok().flatten());
        if exited.is_some() || start.elapsed() > timeout {
            let why = match exited {
                Some(st) => format!("the game exited before it connected ({st})"),
                None => format!("the game did not connect within {} ms", timeout.as_millis()),
            };
            if let Some(c) = &mut session.child {
                let _ = c.kill();
                let _ = c.wait();
            }
            return Ok(fail("attach_failed", format!("{why}. Run `spark check` on the game first; a window needs a GPU.")));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut out = session.describe();
    out["ok"] = true.into();
    out["state"] = "connected".into();
    SESSION.with(|s| *s.borrow_mut() = Some(session));
    Ok(out)
}

fn game_control(a: &Value) -> Result<Value, String> {
    let action = a.get("action").and_then(Value::as_str).ok_or("missing argument: action")?.to_string();
    let frames = a.get("frames").and_then(Value::as_u64).unwrap_or(1).clamp(1, 100_000) as u32;
    let on = a.get("on").and_then(Value::as_bool).unwrap_or(true);
    Ok(with_live(|s| {
        let send = |s: &Session, c: EditorCommand| {
            s.link.as_ref().expect("connected").send(c);
        };
        match action.as_str() {
            "status" | "stats" => {}
            "pause" | "resume" => {
                let want = action == "pause";
                send(s, EditorCommand::Pause { paused: want });
                s.wait(Duration::from_secs(2), |m| matches!(m, GameMessage::Paused { paused } if *paused == want).then_some(()));
            }
            "step" => {
                send(s, EditorCommand::Step { frames });
                s.wait(Duration::from_secs(2), |m| matches!(m, GameMessage::Paused { paused: true }).then_some(()));
            }
            "reload" => send(s, EditorCommand::Reload),
            "overlay" => send(s, EditorCommand::Overlay { on }),
            other => {
                return Err(fail("bad_action", format!("unknown action '{other}' (status, pause, resume, step, reload, overlay, stats)")));
            }
        }
        // Let one stats message arrive so the answer is fresh.
        if matches!(action.as_str(), "stats" | "step" | "reload") {
            s.stats = None;
            s.wait(Duration::from_secs(2), |m| matches!(m, GameMessage::Stats(_)).then_some(()));
        }
        let mut out = s.describe();
        out["ok"] = true.into();
        Ok(out)
    })
    .unwrap_or_else(|e| e))
}

fn detach() -> Result<Value, String> {
    let Some(mut s) = SESSION.with(|s| s.borrow_mut().take()) else { return Ok(json!({ "ok": true, "was_attached": false })) };
    if let Some(l) = &s.link {
        l.send(EditorCommand::Quit);
    }
    let mut exited = false;
    if let Some(c) = &mut s.child {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(3) {
            if c.try_wait().ok().flatten().is_some() {
                exited = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        if !exited {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
    let _ = &s.token;
    Ok(json!({ "ok": true, "was_attached": true, "graceful": exited }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::MeshSource;
    use spark_devtools::{CommandResult, Link, PROTOCOL_VERSION};

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("spark-mcp-scene-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A scene file as the engine writes it.
    fn fixture(dir: &Path) -> (PathBuf, String, String, String) {
        let mut w = World::new();
        let mesh = w.assets.add_mesh_from(&MeshSource::Cube { size: 1.0 }).unwrap();
        let a = w.spawn(mesh, Default::default());
        w.scene[a].name = "Crate".into();
        w.scene[a].transform.position = engine::Vec3::new(1.0, 2.0, 3.0);
        let b = w.spawn(mesh, Default::default());
        w.scene[b].name = "Box".into();
        let id = w.node_id(a).unwrap().to_string();
        let last = w.node_id(b).unwrap().to_string();
        let text = save_scene(&mut w, &mut FsLoader::new(dir, true)).unwrap().text;
        let path = dir.join("level.scene");
        std::fs::write(&path, &text).unwrap();
        (path, text, id, last)
    }

    fn apply(path: &Path, commands: Value) -> Value {
        scene_apply(&json!({ "path": path.display().to_string(), "commands": commands })).unwrap()
    }

    #[test]
    fn offline_invalid_command_is_a_structured_error_and_the_file_is_untouched() {
        let dir = temp("bad");
        let (path, text, id, _) = fixture(&dir);
        let r = apply(&path, json!([{ "op": "set", "node": id, "path": "transform.positon", "value": [0, 0, 0] }]));
        assert_eq!(r["ok"], false, "{r}");
        assert_eq!(r["error"]["code"], "unknown_field", "{r}");
        assert!(r["error"]["path"].is_string() && r["error"]["suggestions"].as_array().unwrap().iter().any(|s| s == "position"), "{r}");
        let r = apply(&path, json!([{ "op": "explode" }]));
        assert_eq!(r["error"]["code"], "bad_command", "{r}");
        assert_eq!(r["error"]["path"], "commands[0].op", "{r}");
        let r = apply(&path, json!([{ "op": "set", "node": "ffffffff", "path": "name", "value": "x" }]));
        assert_eq!(r["error"]["code"], "no_such_node", "{r}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    }

    #[test]
    fn offline_apply_returns_inverses_and_applying_them_restores_the_file() {
        let dir = temp("roundtrip");
        let (path, text, id, last) = fixture(&dir);
        // read back
        let r = scene_read(&json!({ "path": path.display().to_string() })).unwrap();
        assert_eq!(r["nodes"], 2);
        assert_eq!(r["text"], text.as_str());

        let cmds = json!([
            { "op": "set", "node": id, "path": "transform.position", "value": [9, 8, 7] },
            { "op": "set", "node": id, "path": "name", "value": "Renamed" },
            { "op": "spawn", "node": { "name": "Lamp", "components": { "transform": { "position": [0, 2, 0] }, "light": {} } } },
        ]);
        let r = apply(&path, cmds);
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(r["written"], true);
        let changed = std::fs::read_to_string(&path).unwrap();
        assert_ne!(changed, text);
        assert!(changed.contains("Renamed") && changed.contains("Lamp"), "{changed}");
        let inverse = r["inverse"].clone();
        assert_eq!(inverse["op"], "batch");

        let r = apply(&path, json!([inverse]));
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text, "the inverse restored the file exactly");

        // a delete is undone too (the last node: a restored root goes back to the end of the file)
        let r = apply(&path, json!([{ "op": "delete", "node": last }]));
        assert_eq!(r["ok"], true, "{r}");
        assert!(!std::fs::read_to_string(&path).unwrap().contains("Box"));
        let r = apply(&path, json!([r["inverse"].clone()]));
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);

        // dry run changes nothing
        let r = scene_apply(&json!({ "path": path.display().to_string(), "dry_run": true,
            "commands": [{ "op": "set", "node": id, "path": "name", "value": "Q" }] }))
        .unwrap();
        assert_eq!(r["written"], false);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    }

    #[test]
    fn tools_go_through_the_mcp_server() {
        let names: Vec<String> = tools().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
        assert_eq!(names, ["scene_read", "scene_apply", "attach", "game_control", "detach"]);
        let dir = temp("rpc");
        let (path, text, id, _) = fixture(&dir);
        let req = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"scene_apply","arguments":{
            "path": path.display().to_string(), "commands": [{ "op": "set", "node": id, "path": "name", "value": 5 }] }}});
        let r = crate::mcp::handle(&req).unwrap();
        assert_eq!(r["result"]["isError"], true, "{r}");
        let body: Value = serde_json::from_str(r["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(body["error"]["code"], "wrong_type");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    }

    /// A fake game on the other end of the devtools link: answers commands, pause and scene requests.
    fn fake_game(addr: String, token: String) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            let link: Link<GameMessage, EditorCommand> = Link::connect(&addr, Duration::from_secs(5)).unwrap();
            link.send(GameMessage::Hello { protocol: PROTOCOL_VERSION, engine: "test".into(), pid: 1, gpu: "none".into(), token });
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(20) && link.is_alive() {
                for m in link.drain() {
                    match m {
                        EditorCommand::Command { request, json } => {
                            let r = if json["commands"][0]["op"] == "set" {
                                CommandResult {
                                    request,
                                    ok: true,
                                    inverse: Some(json!({"op":"batch","commands":[]})),
                                    changed: vec!["a1b2c3d4".into()],
                                    ..Default::default()
                                }
                            } else {
                                CommandResult::failure(
                                    request,
                                    CommandErrorInfo { code: "bad_command".into(), message: "nope".into(), ..Default::default() },
                                )
                            };
                            link.send(GameMessage::CommandResult(r));
                        }
                        EditorCommand::Pause { paused } => {
                            link.send(GameMessage::Paused { paused });
                        }
                        EditorCommand::WatchScene { interval_ms: Some(_) } => {
                            link.send(GameMessage::Scene(Default::default()));
                        }
                        EditorCommand::Quit => return,
                        _ => {}
                    }
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        })
    }

    #[test]
    fn attach_listen_then_live_commands_pause_and_detach() {
        assert_eq!(game_control(&json!({"action":"status"})).unwrap()["error"]["code"], "not_attached");
        let r = attach(&json!({ "listen": true })).unwrap();
        assert_eq!(r["state"], "waiting");
        let (addr, token) = (r["addr"].as_str().unwrap().to_string(), r["token"].as_str().unwrap().to_string());
        assert_eq!(attach(&json!({ "listen": true })).unwrap()["error"]["code"], "already_attached");
        assert_eq!(scene_read(&json!({})).unwrap()["error"]["code"], "not_connected");
        let game = fake_game(addr, token);
        // wait for the game to connect
        let mut st = Value::Null;
        for _ in 0..200 {
            st = game_control(&json!({"action":"status"})).unwrap();
            if st["ok"] == true {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(st["connected"], true, "{st}");
        assert_eq!(st["protocol"], PROTOCOL_VERSION);

        let ok = scene_apply(&json!({ "commands": [{ "op": "set", "node": "a1b2c3d4", "path": "name", "value": "x" }] })).unwrap();
        assert_eq!(ok["ok"], true, "{ok}");
        assert_eq!(ok["mode"], "live");
        assert_eq!(ok["changed"][0], "a1b2c3d4");
        let bad = scene_apply(&json!({ "commands": [{ "op": "delete", "node": "a1b2c3d4" }] })).unwrap();
        assert_eq!(bad["error"]["code"], "bad_command", "{bad}");
        let live = scene_read(&json!({})).unwrap();
        assert_eq!(live["mode"], "live");
        assert_eq!(game_control(&json!({"action":"pause"})).unwrap()["paused"], true);
        assert_eq!(game_control(&json!({"action":"resume"})).unwrap()["paused"], false);
        assert_eq!(game_control(&json!({"action":"fly"})).unwrap()["error"]["code"], "bad_action");

        let d = detach().unwrap();
        assert_eq!(d["was_attached"], true);
        game.join().unwrap();
        assert_eq!(detach().unwrap()["was_attached"], false);
    }
}
