//! Message types. Everything is `#[serde(tag = "type")]`: `{"type":"stats","frame":42,...}`.

use serde::{Deserialize, Serialize};

/// Game -> editor.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GameMessage {
    /// First message after connecting.
    /// `token` is the value of `SPARK_DEVTOOLS_TOKEN` the editor gave the game (the editor drops connections without the right token, protocol 3+).
    Hello {
        protocol: u32,
        engine: String,
        pid: u32,
        gpu: String,
        #[serde(default)]
        token: String,
    },
    /// Frame numbers, a few times per second.
    Stats(Stats),
    /// The whole scene (see [`SceneSnapshot`]), when the editor subscribed with [`EditorCommand::WatchScene`].
    Scene(SceneSnapshot),
    /// The game logic was paused / resumed (by the editor or the game).
    Paused { paused: bool },
    /// The script host stopped on an error (`None` = the error is gone, e.g. after a hot reload). With a `node_id`
    /// it is not the host: the script of that one object failed (the game keeps running); the editor can mark the
    /// node. Older games never send `node_id`, older editors ignore it (it has a serde default).
    ScriptError {
        message: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        node_id: Option<String>,
    },
    /// The answer to [`EditorCommand::Command`] (protocol 4).
    CommandResult(CommandResult),
    /// Sent right before the game exits.
    Bye,
}

/// Editor -> game.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EditorCommand {
    /// Stop / continue game logic. Rendering continues, so the window stays alive.
    Pause { paused: bool },
    /// While paused: run exactly `frames` frames of logic.
    Step { frames: u32 },
    /// Restart the game (scene cleared, `start()` runs again), like a hot reload.
    Reload,
    /// Send [`GameMessage::Scene`] every `interval_ms` (`None` = stop).
    WatchScene { interval_ms: Option<u32> },
    /// Turn the in-game F1 debug overlay on / off.
    Overlay { on: bool },
    /// Edit one object of the running scene (id = the id of the scene dump). Only the fields that are Some change;
    /// values must be finite. Reverted by the next Reload like every runtime change.
    SetObject {
        id: u32,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        position: Option<[f32; 3]>,
        #[serde(default)]
        rotation_deg: Option<[f32; 3]>,
        #[serde(default)]
        scale: Option<[f32; 3]>,
        #[serde(default)]
        visible: Option<bool>,
    },
    /// Applies one command (protocol 4; the JSON form is in [`crate::command_json`]) to the running scene through
    /// `World::apply`. The game answers with [`GameMessage::CommandResult`] carrying the same `request`.
    /// Like every runtime change it is reverted by the next Reload.
    Command {
        /// Echoed back in the result, so the editor can match answers to requests.
        #[serde(default)]
        request: u64,
        json: serde_json::Value,
    },
    /// Close the game gracefully.
    Quit,
}

/// The game's answer to one [`EditorCommand::Command`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CommandResult {
    pub request: u64,
    pub ok: bool,
    /// On success: a command that undoes this one (apply it with [`EditorCommand::Command`]). It is plain JSON,
    /// or `{"op":"restore","token":N}` when the exact state is kept in the game (see [`crate::command_json`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inverse: Option<serde_json::Value>,
    /// On success: node ids that were created, changed or deleted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed: Vec<String>,
    /// On success, only when the command had implicit parts (generated node ids of a `spawn`): the command with them
    /// filled in. Redo must send this one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<CommandErrorInfo>,
}

/// Why a command was rejected. Nothing was changed.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CommandErrorInfo {
    /// Stable code: `no_such_node`, `unknown_field`, `out_of_range`, `wrong_type`, `bad_command`, ...
    pub code: String,
    pub message: String,
    /// The property path or the place in the command JSON the error is about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Did-you-mean names for an unknown field or component.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suggestions: Vec<String>,
}

impl CommandErrorInfo {
    pub fn from_json_error(e: &crate::command_json::JsonError) -> Self {
        Self { code: e.code.to_string(), message: e.to_string(), path: e.path.clone(), suggestions: Vec::new() }
    }

    pub fn from_command_error(e: &spark_core::CommandError) -> Self {
        use spark_core::CommandError;
        let (path, suggestions) = match e {
            CommandError::Reflect(r) => {
                let s = match r {
                    spark_reflect::ReflectError::UnknownField { suggestions, .. } => suggestions.iter().map(|s| s.to_string()).collect(),
                    _ => Vec::new(),
                };
                (Some(r.path().to_string()), s)
            }
            CommandError::UnknownComponent { name, suggestions } => {
                (Some(name.clone()), suggestions.iter().map(|s| s.to_string()).collect())
            }
            _ => (None, Vec::new()),
        };
        let message = match e {
            CommandError::Reflect(r) => r.to_string(),
            other => other.to_string(),
        };
        Self { code: e.code().to_string(), message, path, suggestions }
    }
}

impl CommandResult {
    pub fn failure(request: u64, error: CommandErrorInfo) -> Self {
        Self { request, ok: false, error: Some(error), ..Self::default() }
    }
}

/// Per-frame numbers.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub frame: u64,
    /// Game time in seconds (scaled, stops while paused).
    pub time: f64,
    pub fps: f32,
    pub frame_ms: f32,
    pub min_frame_ms: f32,
    pub max_frame_ms: f32,
    pub objects: usize,
    pub draw_calls: u32,
    pub culled: u32,
    pub triangles: u64,
    pub internal_size: (u32, u32),
    pub paused: bool,
}

/// The scene as the engine dumps it (`Scene::dump_json`). Unknown fields are ignored, so the engine
/// may add more without breaking older editors.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SceneSnapshot {
    #[serde(default)]
    pub frame: u64,
    pub camera: CameraInfo,
    pub objects: Vec<ObjectInfo>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CameraInfo {
    pub position: [Option<f32>; 3],
    pub forward: [Option<f32>; 3],
    pub fov: f32,
    pub ortho: Option<f32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ObjectInfo {
    pub id: u32,
    /// Stable id for commands (protocol 4; `None` from an older game).
    #[serde(default)]
    pub node_id: Option<String>,
    #[serde(default)]
    pub name: String,
    pub parent: Option<u32>,
    pub mesh: Option<u32>,
    pub position: [Option<f32>; 3],
    pub world_position: [Option<f32>; 3],
    pub rotation_deg: [Option<f32>; 3],
    pub scale: [Option<f32>; 3],
    pub visible: bool,
    pub bounds: Option<[[Option<f32>; 3]; 2]>,
    pub body: Option<BodyInfo>,
    #[serde(default)]
    pub light: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BodyInfo {
    pub kind: String,
    pub velocity: [Option<f32>; 3],
}

impl SceneSnapshot {
    /// Parses the output of `Scene::dump_json`.
    pub fn from_engine_json(frame: u64, json: &str) -> Result<Self, String> {
        let mut s: Self = serde_json::from_str(json).map_err(|e| format!("bad scene json: {e}"))?;
        s.frame = frame;
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let msgs = [
            GameMessage::Hello { protocol: 4, engine: "0.1.0".into(), pid: 7, gpu: "x".into(), token: "t".into() },
            GameMessage::Stats(Stats { frame: 3, fps: 60.0, ..Default::default() }),
            GameMessage::Paused { paused: true },
            GameMessage::ScriptError { message: Some("main.luau:3: boom".into()), node_id: None },
            GameMessage::ScriptError { message: Some("[SPK-1000] script src/door.luau: boom".into()), node_id: Some("1a2b3c4d".into()) },
            GameMessage::Bye,
        ];
        for m in msgs {
            let line = serde_json::to_string(&m).unwrap();
            assert!(!line.contains('\n'));
            assert_eq!(serde_json::from_str::<GameMessage>(&line).unwrap(), m);
        }
        let c = EditorCommand::WatchScene { interval_ms: Some(250) };
        let line = serde_json::to_string(&c).unwrap();
        assert_eq!(line, r#"{"type":"watch_scene","interval_ms":250}"#);
        assert_eq!(serde_json::from_str::<EditorCommand>(&line).unwrap(), c);
        let set = EditorCommand::SetObject {
            id: 4,
            name: None,
            position: Some([1.0, 2.0, 3.0]),
            rotation_deg: None,
            scale: None,
            visible: Some(false),
        };
        assert_eq!(serde_json::from_str::<EditorCommand>(&serde_json::to_string(&set).unwrap()).unwrap(), set);
        // a message from before `node_id` still parses (and is written back without it)
        let old = r#"{"type":"script_error","message":"boom"}"#;
        assert_eq!(
            serde_json::from_str::<GameMessage>(old).unwrap(),
            GameMessage::ScriptError { message: Some("boom".into()), node_id: None }
        );
        // an old game without a token still parses (the editor then rejects it)
        let old = r#"{"type":"hello","protocol":1,"engine":"0.1.0","pid":1,"gpu":"g"}"#;
        assert!(matches!(serde_json::from_str::<GameMessage>(old).unwrap(), GameMessage::Hello { token, .. } if token.is_empty()));
    }

    #[test]
    fn command_messages_round_trip() {
        let c = EditorCommand::Command { request: 7, json: serde_json::json!({"op":"set","node":"a1","path":"body.mass","value":80.0}) };
        let line = serde_json::to_string(&c).unwrap();
        assert_eq!(line, r#"{"type":"command","request":7,"json":{"node":"a1","op":"set","path":"body.mass","value":80.0}}"#);
        assert_eq!(serde_json::from_str::<EditorCommand>(&line).unwrap(), c);
        let ok = GameMessage::CommandResult(CommandResult {
            request: 7,
            ok: true,
            inverse: Some(serde_json::json!({"op":"set","node":"a1","path":"body.mass","value":1.0})),
            changed: vec!["a1".into()],
            ..Default::default()
        });
        let err = GameMessage::CommandResult(CommandResult::failure(
            8,
            CommandErrorInfo {
                code: "unknown_field".into(),
                message: "unknown field `body.mas` on Object; did you mean `mass`?".into(),
                path: Some("body.mas".into()),
                suggestions: vec!["mass".into()],
            },
        ));
        for m in [ok, err] {
            let line = serde_json::to_string(&m).unwrap();
            assert_eq!(serde_json::from_str::<GameMessage>(&line).unwrap(), m);
        }
        let line = serde_json::to_string(&GameMessage::CommandResult(CommandResult::failure(
            1,
            CommandErrorInfo { code: "no_such_node".into(), message: "m".into(), ..Default::default() },
        )))
        .unwrap();
        assert_eq!(line, r#"{"type":"command_result","request":1,"ok":false,"error":{"code":"no_such_node","message":"m"}}"#);
    }

    #[test]
    fn parses_engine_scene_dump() {
        let json = r#"{"camera":{"position":[0.0000,2.0000,5.0000],"forward":[0.0000,0.0000,-1.0000],"fov":60.00,"ortho":null},"objects":[{"id":0,"name":"ground","parent":null,"mesh":1,"position":[0.0,0.0,0.0],"world_position":[0.0,0.0,0.0],"rotation_deg":[0.0,0.0,0.0],"scale":[1.0,1.0,1.0],"visible":true,"bounds":[[-1.0,0.0,-1.0],[1.0,0.0,1.0]],"body":{"kind":"static","velocity":[0.0,0.0,0.0]},"light":false},{"id":1,"name":"","parent":0,"mesh":null,"position":[null,0.0,0.0],"world_position":[0.0,0.0,0.0],"rotation_deg":[0.0,0.0,0.0],"scale":[1.0,1.0,1.0],"visible":false,"bounds":null,"body":null,"light":true}]}"#;
        let s = SceneSnapshot::from_engine_json(9, json).unwrap();
        assert_eq!(s.frame, 9);
        assert_eq!(s.objects.len(), 2);
        assert_eq!(s.objects[1].parent, Some(0));
        assert_eq!(s.objects[1].position[0], None);
        assert_eq!(s.objects[0].body.as_ref().unwrap().kind, "static");
    }
}
