//! `--devtools ADDR`: the runner's side of the live link to Spark Editor (see `spark-devtools`).
//!
//! Every frame the runner calls [`Devtools::begin_frame`] (applies editor commands, decides whether game
//! logic runs) and [`Devtools::end_frame`] (sends stats / scene snapshots, throttled). All socket I/O is
//! on background threads; a missing or crashed editor never affects the game.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use spark_core::{Command, VERSION, World};
use spark_devtools::command_json::{command_from_json, command_to_json, needs_token, widen_ints};
use spark_devtools::{CommandErrorInfo, CommandResult, EditorCommand, GameMessage, Link, PROTOCOL_VERSION, SceneSnapshot, Stats};
use spark_render::FrameStats;

const STATS_EVERY: Duration = Duration::from_millis(100);
const MIN_SCENE_EVERY: Duration = Duration::from_millis(16);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

pub(crate) struct Devtools {
    link: Link<GameMessage, EditorCommand>,
    paused: bool,
    /// Frames of logic still to run while paused (`Step`).
    steps: u32,
    scene_every: Option<Duration>,
    last_scene: Option<Instant>,
    last_stats: Option<Instant>,
    last_error: Option<String>,
    commands: CommandHost,
}

impl Devtools {
    /// Connects to the editor. `None` (with a warning) when it is not reachable: the game runs normally.
    pub fn connect(addr: &str, gpu: &str) -> Option<Self> {
        // Belt and braces: `App::new` already drops the flag for packed games.
        if spark_core::vfs::is_packed() {
            return None;
        }
        match Link::connect(addr, CONNECT_TIMEOUT) {
            Ok(link) => {
                link.send(GameMessage::Hello {
                    protocol: PROTOCOL_VERSION,
                    engine: VERSION.to_string(),
                    pid: std::process::id(),
                    gpu: gpu.to_string(),
                    token: std::env::var(spark_devtools::TOKEN_ENV).unwrap_or_default(),
                });
                log::info!("devtools: connected to {addr}");
                Some(Self {
                    link,
                    paused: false,
                    steps: 0,
                    scene_every: None,
                    last_scene: None,
                    last_stats: None,
                    last_error: None,
                    commands: CommandHost::default(),
                })
            }
            Err(e) => {
                log::warn!("devtools: cannot connect to {addr}: {e}");
                None
            }
        }
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    fn set_paused(&mut self, paused: bool) {
        if self.paused != paused {
            self.paused = paused;
            self.link.send(GameMessage::Paused { paused });
        }
    }

    /// Applies the editor's commands. Returns whether game logic runs this frame.
    pub fn begin_frame(&mut self, world: &mut World) -> bool {
        let commands: Vec<EditorCommand> = self.link.drain().collect();
        for cmd in commands {
            match cmd {
                EditorCommand::Pause { paused } => {
                    self.steps = 0;
                    self.set_paused(paused);
                }
                EditorCommand::Step { frames } => {
                    self.set_paused(true);
                    self.steps = self.steps.saturating_add(frames.max(1));
                }
                EditorCommand::Reload => {
                    world.request_restart();
                    // The script host restarts at the start of a logic frame: run one even while paused.
                    if self.paused {
                        self.steps = self.steps.max(1);
                    }
                }
                EditorCommand::WatchScene { interval_ms } => {
                    self.scene_every = interval_ms.map(|ms| Duration::from_millis(ms as u64).max(MIN_SCENE_EVERY));
                    self.last_scene = None;
                }
                EditorCommand::Overlay { on } => world.debug.set_enabled(on),
                EditorCommand::SetObject { id, name, position, rotation_deg, scale, visible } => {
                    self.commands.set_object(world, id, name, position, rotation_deg, scale, visible);
                }
                EditorCommand::Command { request, json } => {
                    let result = self.commands.run(world, request, &json);
                    self.link.send(GameMessage::CommandResult(result));
                }
                EditorCommand::Quit => world.quit(),
            }
        }
        if !self.paused {
            return true;
        }
        if self.steps > 0 {
            self.steps -= 1;
            return true;
        }
        false
    }

    /// Sends what changed: script errors at once, stats ~10x per second, the scene when watched.
    pub fn end_frame(&mut self, world: &mut World, render: FrameStats, error: Option<&str>) {
        if error != self.last_error.as_deref() {
            self.last_error = error.map(str::to_owned);
            self.link.send(GameMessage::ScriptError { message: self.last_error.clone(), node_id: None });
        }
        // Object scripts that failed: the game goes on, the editor learns which node (one message per failure).
        for f in world.take_script_failures() {
            self.link.send(GameMessage::ScriptError { message: Some(f.message), node_id: f.node_id.map(|n| n.to_string()) });
        }
        let now = Instant::now();
        let due = |last: Option<Instant>, every: Duration| last.is_none_or(|t| now - t >= every);
        if due(self.last_stats, STATS_EVERY) {
            self.last_stats = Some(now);
            let d = &world.debug;
            self.link.send(GameMessage::Stats(Stats {
                frame: world.time.frame,
                time: world.time.elapsed,
                fps: d.avg_fps(),
                frame_ms: d.avg_frame_ms(),
                min_frame_ms: d.min_frame_ms,
                max_frame_ms: d.max_frame_ms,
                objects: world.scene.len(),
                draw_calls: render.draw_calls,
                culled: render.culled,
                triangles: render.triangles,
                internal_size: render.internal_size,
                paused: self.paused,
            }));
        }
        if let Some(every) = self.scene_every {
            if due(self.last_scene, every) {
                self.last_scene = Some(now);
                give_node_ids(world);
                match SceneSnapshot::from_engine_json(world.time.frame, &world.scene.dump_json()) {
                    Ok(scene) => {
                        self.link.send(GameMessage::Scene(scene));
                    }
                    Err(e) => log::warn!("devtools: {e}"),
                }
            }
        }
    }

    /// Best effort "the game is closing" (the editor also notices the process exit).
    pub fn bye(&self) {
        if self.link.send(GameMessage::Bye) {
            std::thread::sleep(Duration::from_millis(30));
        }
    }
}

/// Every object gets a stable node id (the editor addresses commands by it); ids stay for the session.
fn give_node_ids(world: &mut World) {
    let missing: Vec<_> = world.scene.iter().filter(|(_, o)| o.node_id().is_none()).map(|(id, _)| id).collect();
    for id in missing {
        world.node_id(id);
    }
}

/// How many undo tokens (exact inverses kept in the game) are remembered.
const MAX_TOKENS: usize = 256;

/// Applies the editor's commands to the world and remembers the inverses that have no JSON form (see
/// `spark_devtools::command_json`: `Restore` and the `Spawn` that undoes a `Delete`).
#[derive(Default)]
pub(crate) struct CommandHost {
    next_token: u64,
    stored: VecDeque<(u64, Command)>,
}

impl CommandHost {
    /// Applies one JSON command and builds the answer.
    pub fn run(&mut self, world: &mut World, request: u64, json: &serde_json::Value) -> CommandResult {
        let cmd = match command_from_json(json, &|t| self.stored.iter().find(|(k, _)| *k == t).map(|(_, c)| c.clone())) {
            Ok(c) => c,
            Err(e) => return CommandResult::failure(request, CommandErrorInfo::from_json_error(&e)),
        };
        let mut result = world.apply(&cmd);
        if matches!(&result, Err(e) if e.code() == "wrong_type") {
            // JSON has one number type: `5` is an integer, and a float field rejects it. Retry with floats.
            if let Some(widened) = widen_ints(&cmd) {
                if let Ok(a) = world.apply(&widened) {
                    result = Ok(a);
                }
            }
        }
        match result {
            Err(e) => CommandResult::failure(request, CommandErrorInfo::from_command_error(&e)),
            Ok(applied) => CommandResult {
                request,
                ok: true,
                inverse: Some(self.encode(&applied.inverse, true)),
                changed: applied.changed.iter().map(|n| n.to_string()).collect(),
                resolved: applied.resolved.as_ref().map(|c| self.encode(c, false)),
                error: None,
            },
        }
    }

    /// The JSON of a command; with `exact` (inverses), commands whose exact form matters become a token.
    fn encode(&mut self, cmd: &Command, exact: bool) -> serde_json::Value {
        if !(exact && needs_token(cmd)) {
            if let Ok(j) = command_to_json(cmd) {
                return j;
            }
        }
        let token = self.next_token;
        self.next_token += 1;
        self.stored.push_back((token, cmd.clone()));
        if self.stored.len() > MAX_TOKENS {
            self.stored.pop_front();
        }
        serde_json::json!({ "op": "restore", "token": token })
    }

    /// `SetObject` (protocol 2 and 3): sugar for a batch of `SetProperty`. Non-finite vectors and unknown ids are
    /// ignored with a warning, as before.
    #[allow(clippy::too_many_arguments)]
    pub fn set_object(
        &mut self,
        world: &mut World,
        id: u32,
        name: Option<String>,
        position: Option<[f32; 3]>,
        rotation_deg: Option<[f32; 3]>,
        scale: Option<[f32; 3]>,
        visible: Option<bool>,
    ) {
        use spark_reflect::Value;
        let finite = |v: &[f32; 3]| v.iter().all(|x| x.is_finite());
        let Some(oid) = world.scene.id_of_index(id) else {
            log::warn!("devtools: SetObject: no object #{id}");
            return;
        };
        let Some(node) = world.node_id(oid) else { return };
        let set = |path: &str, value: Value| Command::SetProperty { node: node.clone(), path: path.to_string(), value };
        let mut batch = Vec::new();
        if let Some(n) = name {
            batch.push(set("name", Value::Str(n)));
        }
        if let Some(p) = position.filter(finite) {
            batch.push(set("transform.position", Value::Vec3(p)));
        }
        if let Some(r) = rotation_deg.filter(finite) {
            batch.push(set("transform.rotation", Value::Vec3(r)));
        }
        if let Some(s) = scale.filter(finite) {
            batch.push(set("transform.scale", Value::Vec3(s)));
        }
        if let Some(v) = visible {
            batch.push(set("visible", Value::Bool(v)));
        }
        if let Err(e) = world.apply(&Command::Batch(batch)) {
            log::warn!("devtools: SetObject #{id}: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value as Json, json};
    use spark_core::{Body, MeshSource, Vec3};

    /// What `SetObject` did before protocol 4 (direct field writes), kept as the reference.
    fn legacy_set_object(
        world: &mut World,
        id: u32,
        name: Option<String>,
        position: Option<[f32; 3]>,
        rotation_deg: Option<[f32; 3]>,
        scale: Option<[f32; 3]>,
        visible: Option<bool>,
    ) {
        use spark_core::glam::{EulerRot, Quat};
        let finite = |v: &[f32; 3]| v.iter().all(|x| x.is_finite());
        let oid = world.scene.id_of_index(id).unwrap();
        let o = world.scene.get_mut(oid).unwrap();
        if let Some(n) = name {
            o.name = n;
        }
        if let Some(p) = position.filter(finite) {
            o.transform.position = Vec3::from_array(p);
        }
        if let Some(r) = rotation_deg.filter(finite) {
            o.transform.rotation = Quat::from_euler(EulerRot::YXZ, r[1].to_radians(), r[0].to_radians(), r[2].to_radians());
        }
        if let Some(s) = scale.filter(finite) {
            o.transform.scale = Vec3::from_array(s);
        }
        if let Some(v) = visible {
            o.visible = v;
        }
    }

    fn world_with_crate() -> (World, String) {
        let mut w = World::new();
        let mesh = w.assets.add_mesh_from(&MeshSource::Cube { size: 1.0 }).unwrap();
        let oid = w.spawn(mesh, Default::default());
        w.scene[oid].body = Some(Body::default());
        let id = w.node_id(oid).unwrap().to_string();
        (w, id)
    }

    fn run(host: &mut CommandHost, w: &mut World, json: Json) -> CommandResult {
        host.run(w, 1, &json)
    }

    #[test]
    fn set_object_matches_the_old_direct_writes() {
        type Case = (Option<&'static str>, Option<[f32; 3]>, Option<[f32; 3]>, Option<[f32; 3]>, Option<bool>);
        let cases: [Case; 4] = [
            (Some("renamed"), Some([1.0, 2.0, 3.0]), Some([10.0, 20.0, 30.0]), Some([2.0, 0.0, -1.0]), Some(false)),
            (None, Some([f32::NAN, 0.0, 0.0]), Some([0.0, 90.0, 0.0]), None, None),
            (None, None, Some([f32::INFINITY, 0.0, 0.0]), Some([1.0, 1.0, 1.0]), Some(true)),
            (None, None, None, None, None),
        ];
        for (name, pos, rot, scale, vis) in cases {
            let (mut a, _) = world_with_crate();
            let (mut b, _) = world_with_crate();
            let id = a.scene.iter().next().unwrap().0.index();
            legacy_set_object(&mut a, id, name.map(String::from), pos, rot, scale, vis);
            CommandHost::default().set_object(&mut b, id, name.map(String::from), pos, rot, scale, vis);
            let (oa, ob) = (&a.scene[a.scene.id_of_index(id).unwrap()], &b.scene[b.scene.id_of_index(id).unwrap()]);
            assert_eq!((&oa.name, oa.visible, oa.transform), (&ob.name, ob.visible, ob.transform), "{name:?} {pos:?} {rot:?}");
        }
        // unknown object: nothing happens
        let (mut w, _) = world_with_crate();
        CommandHost::default().set_object(&mut w, 999, Some("x".into()), None, None, None, None);
    }

    #[test]
    fn set_property_changes_mass_and_returns_the_inverse() {
        let (mut w, id) = world_with_crate();
        let mut host = CommandHost::default();
        let r = run(&mut host, &mut w, json!({"op":"set","node":id,"path":"body.mass","value":80.0}));
        assert!(r.ok, "{r:?}");
        assert_eq!(r.changed, vec![id.clone()]);
        let oid = w.scene.iter().next().unwrap().0;
        assert_eq!(w.scene[oid].body.as_ref().unwrap().mass, Some(80.0));
        assert_eq!(r.inverse.as_ref().unwrap()["op"], "set");
        assert_eq!(r.inverse.as_ref().unwrap()["path"], "body.mass");
        // a whole number is accepted for a float field
        let r = run(&mut host, &mut w, json!({"op":"set","node":id,"path":"body.mass","value":5}));
        assert!(r.ok, "{r:?}");
        assert_eq!(w.scene[oid].body.as_ref().unwrap().mass, Some(5.0));
        // undo with the inverse sent back
        let inv = r.inverse.unwrap();
        assert!(run(&mut host, &mut w, inv).ok);
        assert_eq!(w.scene[oid].body.as_ref().unwrap().mass, Some(80.0));
    }

    #[test]
    fn rejected_commands_answer_with_a_code_and_change_nothing() {
        let (mut w, id) = world_with_crate();
        let mut host = CommandHost::default();
        let oid = w.scene.iter().next().unwrap().0;
        let before = w.scene[oid].body.as_ref().unwrap().mass;

        let r = run(&mut host, &mut w, json!({"op":"set","node":id,"path":"body.mass","value":-3.0}));
        assert!(!r.ok);
        let e = r.error.unwrap();
        assert_eq!(e.code, "out_of_range", "{e:?}");
        assert_eq!(e.path.as_deref(), Some("body.mass"));
        assert_eq!(w.scene[oid].body.as_ref().unwrap().mass, before);

        let r = run(&mut host, &mut w, json!({"op":"set","node":id,"path":"body.mas","value":3.0}));
        let e = r.error.unwrap();
        assert_eq!(e.code, "unknown_field");
        assert_eq!(e.suggestions, vec!["mass".to_string()]);
        assert!(e.message.contains("did you mean `mass`"), "{}", e.message);

        let r = run(&mut host, &mut w, json!({"op":"set","node":"ffffffff","path":"name","value":"x"}));
        assert_eq!(r.error.unwrap().code, "no_such_node");

        let r = run(&mut host, &mut w, json!({"op":"explode"}));
        assert_eq!(r.error.unwrap().code, "bad_command");
        let r = run(&mut host, &mut w, json!({"op":"restore","token":99}));
        assert_eq!(r.error.unwrap().code, "unknown_token");
    }

    #[test]
    fn delete_is_undone_through_a_token() {
        let (mut w, id) = world_with_crate();
        let mut host = CommandHost::default();
        let r = run(&mut host, &mut w, json!({"op":"delete","node":id}));
        assert!(r.ok, "{r:?}");
        assert_eq!(w.scene.len(), 0);
        let inv = r.inverse.unwrap();
        assert_eq!(inv["op"], "restore");
        let r = run(&mut host, &mut w, inv);
        assert!(r.ok, "{r:?}");
        assert_eq!(w.scene.len(), 1);
        assert_eq!(r.inverse.unwrap()["op"], "delete");
        assert!(w.find_node(&spark_core::NodeId::new(&id).unwrap()).is_some(), "the node id survives");
    }

    #[test]
    fn spawn_from_json_resolves_generated_ids() {
        let mut w = World::new();
        let mut host = CommandHost::default();
        let cmd = json!({"op":"spawn","node":{"name":"Lamp","components":{"transform":{"position":[0.0,2.0,0.0]},"light":{}}}});
        let r = run(&mut host, &mut w, cmd);
        assert!(r.ok, "{r:?}");
        assert_eq!(w.scene.len(), 1);
        let resolved = r.resolved.unwrap();
        assert!(resolved["node"]["id"].is_string(), "{resolved}");
        assert_eq!(r.inverse.unwrap()["op"], "delete");
    }
}
