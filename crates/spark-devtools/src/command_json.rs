//! The JSON form of a [`Command`] (ADR 0005, devtools protocol 4).
//!
//! ```json
//! {"op":"set","node":"a1f3c09e","path":"body.mass","value":80.0}
//! {"op":"spawn","node":{"name":"Crate","components":{"transform":{"position":[0,1,0]}}},"parent":null,"index":null}
//! {"op":"batch","commands":[{"op":"delete","node":"a1f3c09e"},{"op":"reparent","node":"b2","parent":"c3"}]}
//! ```
//!
//! `op` is one of `spawn`, `delete`, `set`, `reparent`, `add_component`, `remove_component`, `batch`, `restore`.
//!
//! Property values are [`spark_reflect::Value`]s in a natural JSON form ([`value_to_json`] / [`value_from_json`]):
//!
//! | JSON | Value |
//! |---|---|
//! | `true` | `Bool` |
//! | `5` (no fraction) | `Int` (for integer fields) |
//! | `5.0`, `0.25` | `Float` (for float fields: write the `.0`) |
//! | `"text"` | `Str` (also a unit enum variant) |
//! | `[x, y]`, `[x, y, z]`, `[x, y, z, w]` of numbers | `Vec2`, `Vec3`, `Vec4` (a color is a `Vec4`) |
//! | any other array | `List` |
//! | `{"$variant":"cylinder","radius":0.5}` | `Enum` |
//! | `{"$color":[r,g,b,a]}`, `{"$node":"id"}`, `{"$asset":"path"}` | `Color`, `NodeRef`, `Asset` |
//! | other objects | `Map` |
//! | `null` | `None` |
//!
//! `Restore` carries exact in-process state (a quaternion bit for bit, texture and mesh handles) and has no JSON form:
//! the game keeps those commands and hands out `{"op":"restore","token":N}` instead. [`command_from_json`] asks a
//! resolver for the stored command, so a token is accepted anywhere a command is, also inside a `batch`.

use serde_json::{Map, Number, Value as Json};
use spark_core::{Command, MeshDoc, MeshSource, MeshState, NodeDoc, NodeId, Object, SceneDoc, registry::registry};
use spark_reflect::{ReflectEnum, Value};

/// Why a JSON command could not be read (or written).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonError {
    /// Stable code: `bad_command`, `invalid_node_id`, `unknown_token`, `not_representable`.
    pub code: &'static str,
    pub message: String,
    /// Where in the command the problem is, e.g. `commands[1].node`.
    pub path: Option<String>,
}

impl JsonError {
    fn new(code: &'static str, path: &str, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), path: (!path.is_empty()).then(|| path.to_string()) }
    }
}

impl std::fmt::Display for JsonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.path {
            Some(p) => write!(f, "{p}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for JsonError {}

type Res<T> = Result<T, JsonError>;

// ---------------------------------------------------------------------------------------------------------
// Values

fn f32_json(x: f32) -> Json {
    // The shortest text that reads back as the same f32 (0.1 stays 0.1 and not 0.10000000149011612).
    x.to_string().parse::<f64>().ok().and_then(Number::from_f64).map_or(Json::Null, Json::Number)
}

fn array(a: &[f32]) -> Json {
    Json::Array(a.iter().copied().map(f32_json).collect())
}

/// The JSON form of a value (see the module docs).
pub fn value_to_json(v: &Value) -> Json {
    match v {
        Value::Bool(b) => Json::Bool(*b),
        Value::Int(i) => Json::Number((*i).into()),
        Value::Float(f) => Number::from_f64(*f).map_or(Json::Null, Json::Number),
        Value::Str(s) => Json::String(s.clone()),
        Value::Vec2(a) => array(a),
        Value::Vec3(a) => array(a),
        Value::Vec4(a) => array(a),
        Value::Color(a) => tagged("$color", array(a)),
        Value::Enum { variant, fields } => {
            let mut m = Map::new();
            m.insert("$variant".into(), Json::String(variant.clone()));
            for (k, f) in fields {
                m.insert(k.clone(), value_to_json(f));
            }
            Json::Object(m)
        }
        Value::List(l) => Json::Array(l.iter().map(value_to_json).collect()),
        Value::Map(pairs) => Json::Object(pairs.iter().map(|(k, v)| (k.clone(), value_to_json(v))).collect()),
        Value::NodeRef(s) => tagged("$node", Json::String(s.clone())),
        Value::Asset(s) => tagged("$asset", Json::String(s.clone())),
        Value::None => Json::Null,
    }
}

fn tagged(key: &str, v: Json) -> Json {
    let mut m = Map::new();
    m.insert(key.into(), v);
    Json::Object(m)
}

/// Reads the JSON form of a value. Never fails on shape (anything is some value); the registry validates types.
pub fn value_from_json(j: &Json) -> Res<Value> {
    value_at(j, "value")
}

fn floats<const N: usize>(a: &[Json]) -> Option<[f32; N]> {
    let mut out = [0.0; N];
    for (o, j) in out.iter_mut().zip(a) {
        *o = j.as_f64()? as f32;
    }
    Some(out)
}

fn value_at(j: &Json, at: &str) -> Res<Value> {
    Ok(match j {
        Json::Null => Value::None,
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) => match n.as_i64() {
            Some(i) => Value::Int(i),
            None => Value::Float(n.as_f64().unwrap_or(f64::NAN)),
        },
        Json::String(s) => Value::Str(s.clone()),
        Json::Array(a) => {
            let numeric = a.iter().all(Json::is_number);
            match (numeric, a.len()) {
                (true, 2) => Value::Vec2(floats(a).expect("numbers")),
                (true, 3) => Value::Vec3(floats(a).expect("numbers")),
                (true, 4) => Value::Vec4(floats(a).expect("numbers")),
                _ => Value::List(a.iter().enumerate().map(|(i, x)| value_at(x, &format!("{at}[{i}]"))).collect::<Res<_>>()?),
            }
        }
        Json::Object(m) => {
            if let Some(Json::String(variant)) = m.get("$variant") {
                let fields = m
                    .iter()
                    .filter(|(k, _)| *k != "$variant")
                    .map(|(k, v)| Ok((k.clone(), value_at(v, &format!("{at}.{k}"))?)))
                    .collect::<Res<_>>()?;
                return Ok(Value::Enum { variant: variant.clone(), fields });
            }
            if m.len() == 1 {
                let (k, v) = m.iter().next().expect("one entry");
                match (k.as_str(), v) {
                    ("$color", Json::Array(a)) => {
                        let c = floats::<4>(a).filter(|_| a.len() == 4);
                        return c.map(Value::Color).ok_or_else(|| JsonError::new("bad_command", at, "`$color` is [r, g, b, a]"));
                    }
                    ("$node", Json::String(s)) => return Ok(Value::NodeRef(s.clone())),
                    ("$asset", Json::String(s)) => return Ok(Value::Asset(s.clone())),
                    _ => {}
                }
            }
            Value::Map(m.iter().map(|(k, v)| Ok((k.clone(), value_at(v, &format!("{at}.{k}"))?))).collect::<Res<_>>()?)
        }
    })
}

// ---------------------------------------------------------------------------------------------------------
// Nodes

/// A node (and its children) as JSON: `{"id","name","visible","components":{...},"lod":[...],"children":[...]}`.
fn node_to_json(n: &NodeDoc) -> Json {
    let mut o = Object::empty();
    o.transform = n.transform;
    o.material = n.material;
    o.body = n.body.clone();
    o.light = n.light;
    let mut comps = Map::new();
    for c in registry().components {
        if c.name != "mesh" {
            if let Some(v) = (c.get)(&o) {
                comps.insert(c.name.to_string(), value_to_json(&v));
            }
        }
    }
    let mut m = Map::new();
    if let Some(id) = &n.id {
        m.insert("id".into(), Json::String(id.to_string()));
    }
    m.insert("name".into(), Json::String(n.name.clone()));
    m.insert("visible".into(), Json::Bool(n.visible));
    if let Some(ms) = &n.mesh {
        comps.insert("mesh".into(), value_to_json(&ms.mesh.source.to_value()));
        if !ms.lod.is_empty() {
            let lod = ms.lod.iter().map(|(d, m)| {
                let mut e = Map::new();
                e.insert("distance".into(), f32_json(*d));
                e.insert("mesh".into(), value_to_json(&m.source.to_value()));
                Json::Object(e)
            });
            m.insert("lod".into(), Json::Array(lod.collect()));
        }
    }
    m.insert("components".into(), Json::Object(comps));
    if !n.children.is_empty() {
        m.insert("children".into(), Json::Array(n.children.iter().map(node_to_json).collect()));
    }
    Json::Object(m)
}

fn reflect_err(at: &str, e: spark_reflect::ReflectError) -> JsonError {
    JsonError::new("bad_command", at, e.to_string())
}

fn mesh_doc(j: &Json, at: &str) -> Res<MeshDoc> {
    let v = value_at(j, at)?;
    MeshSource::from_value(&v).map(MeshDoc::from_source).map_err(|e| reflect_err(at, e))
}

fn node_from_json(j: &Json, at: &str) -> Res<NodeDoc> {
    let Json::Object(m) = j else { return Err(JsonError::new("bad_command", at, "a node is an object")) };
    for k in m.keys() {
        if !["id", "name", "visible", "components", "lod", "children"].contains(&k.as_str()) {
            return Err(JsonError::new(
                "bad_command",
                &format!("{at}.{k}"),
                "unknown node field (id, name, visible, components, lod, children)",
            ));
        }
    }
    let mut n = NodeDoc::default();
    if let Some(id) = m.get("id").filter(|v| !v.is_null()) {
        n.id = Some(node_id(id, &format!("{at}.id"))?);
    }
    if let Some(name) = m.get("name") {
        n.name = name.as_str().ok_or_else(|| JsonError::new("bad_command", &format!("{at}.name"), "expected a string"))?.to_string();
    }
    if let Some(v) = m.get("visible") {
        n.visible = v.as_bool().ok_or_else(|| JsonError::new("bad_command", &format!("{at}.visible"), "expected true or false"))?;
    }
    let mut o = Object::empty();
    let mut mesh = None;
    if let Some(Json::Object(comps)) = m.get("components") {
        let reg = registry();
        for (name, cj) in comps {
            let cat = format!("{at}.components.{name}");
            let Some(info) = reg.component(name) else {
                let hint = spark_reflect::did_you_mean(name, reg.components.iter().map(|c| c.name));
                let hint = if hint.is_empty() { String::new() } else { format!(" (did you mean {}?)", hint.join(", ")) };
                return Err(JsonError::new("bad_command", &cat, format!("unknown component '{name}'{hint}")));
            };
            if name == "mesh" {
                mesh = Some(mesh_doc(cj, &cat)?);
                continue;
            }
            let v = value_at(cj, &cat)?;
            let res = if info.required { (info.set)(&mut o, "", v) } else { (info.add)(&mut o, Some(&v)) };
            res.map_err(|e| reflect_err(&cat, e))?;
        }
    } else if m.get("components").is_some_and(|c| !c.is_null()) {
        return Err(JsonError::new("bad_command", &format!("{at}.components"), "expected an object"));
    }
    n.transform = o.transform;
    n.material = o.material;
    n.body = o.body;
    n.light = o.light;
    if let Some(mesh) = mesh {
        let mut lod = Vec::new();
        if let Some(Json::Array(levels)) = m.get("lod") {
            for (i, l) in levels.iter().enumerate() {
                let lat = format!("{at}.lod[{i}]");
                let d = l
                    .get("distance")
                    .and_then(Json::as_f64)
                    .ok_or_else(|| JsonError::new("bad_command", &lat, "`distance` is a number"))?;
                let src = l.get("mesh").ok_or_else(|| JsonError::new("bad_command", &lat, "missing `mesh`"))?;
                lod.push((d as f32, mesh_doc(src, &format!("{lat}.mesh"))?));
            }
        }
        n.mesh = Some(MeshState { mesh, lod });
    }
    if let Some(Json::Array(kids)) = m.get("children") {
        for (i, k) in kids.iter().enumerate() {
            n.children.push(node_from_json(k, &format!("{at}.children[{i}]"))?);
        }
    }
    Ok(n)
}

// ---------------------------------------------------------------------------------------------------------
// Commands

fn opt_id(id: &Option<NodeId>) -> Json {
    id.as_ref().map_or(Json::Null, |i| Json::String(i.to_string()))
}

fn opt_index(i: &Option<usize>) -> Json {
    i.map_or(Json::Null, |i| Json::Number(i.into()))
}

/// The JSON form of a command. `Restore` has none (the game hands out a token instead): `not_representable`.
pub fn command_to_json(cmd: &Command) -> Res<Json> {
    let mut m = Map::new();
    let mut put = |k: &str, v: Json| {
        m.insert(k.to_string(), v);
    };
    match cmd {
        Command::Spawn { subtree, parent, index } => {
            put("op", "spawn".into());
            put("node", node_to_json(&subtree.root));
            put("parent", opt_id(parent));
            put("index", opt_index(index));
        }
        Command::Delete { node } => {
            put("op", "delete".into());
            put("node", node.to_string().into());
        }
        Command::SetProperty { node, path, value } => {
            put("op", "set".into());
            put("node", node.to_string().into());
            put("path", path.clone().into());
            put("value", value_to_json(value));
        }
        Command::Reparent { node, parent, index } => {
            put("op", "reparent".into());
            put("node", node.to_string().into());
            put("parent", opt_id(parent));
            put("index", opt_index(index));
        }
        Command::AddComponent { node, component, value } => {
            put("op", "add_component".into());
            put("node", node.to_string().into());
            put("component", component.clone().into());
            put("value", value.as_ref().map_or(Json::Null, value_to_json));
        }
        Command::RemoveComponent { node, component } => {
            put("op", "remove_component".into());
            put("node", node.to_string().into());
            put("component", component.clone().into());
        }
        Command::Restore { .. } => {
            return Err(JsonError::new("not_representable", "", "a restore command holds exact in-process state and has no JSON form"));
        }
        Command::Batch(cmds) => {
            put("op", "batch".into());
            put("commands", Json::Array(cmds.iter().map(command_to_json).collect::<Res<_>>()?));
        }
    }
    Ok(Json::Object(m))
}

fn node_id(j: &Json, at: &str) -> Res<NodeId> {
    let s = j.as_str().ok_or_else(|| JsonError::new("invalid_node_id", at, "a node id is a string"))?;
    NodeId::new(s).map_err(|e| JsonError::new("invalid_node_id", at, e.to_string()))
}

fn field<'a>(m: &'a Map<String, Json>, k: &str, at: &str) -> Res<&'a Json> {
    m.get(k).ok_or_else(|| JsonError::new("bad_command", at, format!("missing `{k}`")))
}

fn string(m: &Map<String, Json>, k: &str, at: &str) -> Res<String> {
    let kat = join(at, k);
    field(m, k, at)?.as_str().map(str::to_string).ok_or_else(|| JsonError::new("bad_command", &kat, "expected a string"))
}

fn join(at: &str, k: &str) -> String {
    if at.is_empty() { k.to_string() } else { format!("{at}.{k}") }
}

fn node_field(m: &Map<String, Json>, k: &str, at: &str) -> Res<NodeId> {
    node_id(field(m, k, at)?, &join(at, k))
}

fn opt_node_field(m: &Map<String, Json>, k: &str, at: &str) -> Res<Option<NodeId>> {
    match m.get(k) {
        None | Some(Json::Null) => Ok(None),
        Some(j) => node_id(j, &join(at, k)).map(Some),
    }
}

fn opt_index_field(m: &Map<String, Json>, k: &str, at: &str) -> Res<Option<usize>> {
    match m.get(k) {
        None | Some(Json::Null) => Ok(None),
        Some(j) => j
            .as_u64()
            .map(|i| Some(i as usize))
            .ok_or_else(|| JsonError::new("bad_command", &join(at, k), "expected a non-negative integer")),
    }
}

/// Reads a command. `resolve` maps a `restore` token to the command the game stored for it.
pub fn command_from_json(j: &Json, resolve: &dyn Fn(u64) -> Option<Command>) -> Res<Command> {
    command_at(j, "", resolve)
}

fn command_at(j: &Json, at: &str, resolve: &dyn Fn(u64) -> Option<Command>) -> Res<Command> {
    let Json::Object(m) = j else { return Err(JsonError::new("bad_command", at, "a command is an object with an `op`")) };
    let op = string(m, "op", at)?;
    Ok(match op.as_str() {
        "spawn" => Command::Spawn {
            subtree: SceneDoc::new(node_from_json(field(m, "node", at)?, &join(at, "node"))?),
            parent: opt_node_field(m, "parent", at)?,
            index: opt_index_field(m, "index", at)?,
        },
        "delete" => Command::Delete { node: node_field(m, "node", at)? },
        "set" => Command::SetProperty {
            node: node_field(m, "node", at)?,
            path: string(m, "path", at)?,
            value: value_at(field(m, "value", at)?, &join(at, "value"))?,
        },
        "reparent" => Command::Reparent {
            node: node_field(m, "node", at)?,
            parent: opt_node_field(m, "parent", at)?,
            index: opt_index_field(m, "index", at)?,
        },
        "add_component" => Command::AddComponent {
            node: node_field(m, "node", at)?,
            component: string(m, "component", at)?,
            value: match m.get("value") {
                None | Some(Json::Null) => None,
                Some(v) => Some(value_at(v, &join(at, "value"))?),
            },
        },
        "remove_component" => Command::RemoveComponent { node: node_field(m, "node", at)?, component: string(m, "component", at)? },
        "batch" => {
            let Json::Array(list) = field(m, "commands", at)? else {
                return Err(JsonError::new("bad_command", &join(at, "commands"), "expected an array of commands"));
            };
            let mut out = Vec::with_capacity(list.len());
            for (i, c) in list.iter().enumerate() {
                out.push(command_at(c, &format!("{}[{i}]", join(at, "commands")), resolve)?);
            }
            Command::Batch(out)
        }
        "restore" => {
            let token = field(m, "token", at)?
                .as_u64()
                .ok_or_else(|| JsonError::new("bad_command", &join(at, "token"), "expected a non-negative integer"))?;
            resolve(token).ok_or_else(|| {
                JsonError::new("unknown_token", &join(at, "token"), format!("restore token {token} is unknown or expired"))
            })?
        }
        other => {
            let ops = ["spawn", "delete", "set", "reparent", "add_component", "remove_component", "batch", "restore"];
            let hint = spark_reflect::did_you_mean(other, ops);
            let hint =
                if hint.is_empty() { format!(" (one of {})", ops.join(", ")) } else { format!(" (did you mean {}?)", hint.join(", ")) };
            return Err(JsonError::new("bad_command", &join(at, "op"), format!("unknown op '{other}'{hint}")));
        }
    })
}

/// Whether the exact form of the command matters for undo: it contains a `Restore` or a `Spawn` (the inverse of a
/// `Delete` carries the captured subtree with texture and mesh handles). The game keeps such inverses and hands out a
/// token instead of their JSON.
pub fn needs_token(cmd: &Command) -> bool {
    match cmd {
        Command::Restore { .. } | Command::Spawn { .. } => true,
        Command::Batch(v) => v.iter().any(needs_token),
        _ => false,
    }
}

/// The command with whole numbers in values turned into floats (`Some` only if something changed). JSON has one
/// number type, so `"value": 5` reads as an integer; a float field rejects that, and this lets the game retry.
pub fn widen_ints(cmd: &Command) -> Option<Command> {
    fn widen(v: &Value) -> Option<Value> {
        match v {
            Value::Int(i) => Some(Value::Float(*i as f64)),
            Value::List(l) => {
                let new: Vec<Option<Value>> = l.iter().map(widen).collect();
                new.iter()
                    .any(Option::is_some)
                    .then(|| Value::List(l.iter().zip(new).map(|(o, n)| n.unwrap_or_else(|| o.clone())).collect()))
            }
            Value::Map(p) => {
                let new: Vec<Option<Value>> = p.iter().map(|(_, v)| widen(v)).collect();
                new.iter()
                    .any(Option::is_some)
                    .then(|| Value::Map(p.iter().zip(new).map(|((k, o), n)| (k.clone(), n.unwrap_or_else(|| o.clone()))).collect()))
            }
            _ => None,
        }
    }
    match cmd {
        Command::SetProperty { node, path, value } => {
            widen(value).map(|value| Command::SetProperty { node: node.clone(), path: path.clone(), value })
        }
        Command::AddComponent { node, component, value: Some(v) } => {
            widen(v).map(|v| Command::AddComponent { node: node.clone(), component: component.clone(), value: Some(v) })
        }
        Command::Batch(v) => {
            let new: Vec<Option<Command>> = v.iter().map(widen_ints).collect();
            new.iter()
                .any(Option::is_some)
                .then(|| Command::Batch(v.iter().zip(new).map(|(o, n)| n.unwrap_or_else(|| o.clone())).collect()))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spark_core::{Body, ComponentState, Light, Transform, glam::Vec3};

    fn nid(s: &str) -> NodeId {
        NodeId::new(s).unwrap()
    }

    fn none(_: u64) -> Option<Command> {
        None
    }

    fn round_trip(c: &Command) -> Command {
        let j = command_to_json(c).unwrap();
        // through text, like on the wire
        let j: Json = serde_json::from_str(&serde_json::to_string(&j).unwrap()).unwrap();
        command_from_json(&j, &none).unwrap()
    }

    #[test]
    fn every_command_with_a_json_form_round_trips() {
        let mut crate_node = NodeDoc::named("Crate");
        crate_node.id = Some(nid("crate1"));
        crate_node.transform = Transform { position: Vec3::new(1.0, 0.1, -3.5), ..Transform::IDENTITY };
        crate_node.body = Some(Body::default());
        crate_node.light = Some(Light::default());
        crate_node.mesh = Some(MeshState { mesh: MeshDoc::from_source(MeshSource::Cube { size: 2.0 }), lod: Vec::new() });
        let mut child = NodeDoc::named("Child");
        child.visible = false;
        crate_node.children.push(child);
        let cmds = [
            Command::Spawn { subtree: SceneDoc::new(crate_node), parent: Some(nid("root")), index: Some(2) },
            Command::Spawn { subtree: SceneDoc::new(NodeDoc::named("x")), parent: None, index: None },
            Command::Delete { node: nid("a1") },
            Command::SetProperty { node: nid("a1"), path: "body.mass".into(), value: Value::Float(80.0) },
            Command::SetProperty { node: nid("a1"), path: "transform.position".into(), value: Value::Vec3([1.0, 2.0, 0.1]) },
            Command::SetProperty { node: nid("a1"), path: "name".into(), value: Value::Str("hi".into()) },
            Command::SetProperty { node: nid("a1"), path: "material.shader".into(), value: Value::None },
            Command::Reparent { node: nid("a1"), parent: Some(nid("b2")), index: Some(0) },
            Command::Reparent { node: nid("a1"), parent: None, index: None },
            Command::AddComponent { node: nid("a1"), component: "body".into(), value: None },
            Command::AddComponent {
                node: nid("a1"),
                component: "mesh".into(),
                value: Some(Value::Enum { variant: "cylinder".into(), fields: vec![("radius".into(), Value::Float(0.5))] }),
            },
            Command::RemoveComponent { node: nid("a1"), component: "light".into() },
            Command::Batch(vec![
                Command::Delete { node: nid("a1") },
                Command::Batch(vec![Command::SetProperty { node: nid("b"), path: "visible".into(), value: Value::Bool(false) }]),
            ]),
        ];
        for c in &cmds {
            assert_eq!(&round_trip(c), c, "{c:?}");
        }
    }

    #[test]
    fn restore_has_no_json_form_but_a_token_resolves_anywhere() {
        let restore = Command::Restore { node: nid("a1"), state: ComponentState::Transform(Transform::IDENTITY) };
        assert_eq!(command_to_json(&restore).unwrap_err().code, "not_representable");
        assert_eq!(command_to_json(&Command::Batch(vec![restore.clone()])).unwrap_err().code, "not_representable");
        assert!(needs_token(&Command::Batch(vec![restore.clone()])));
        assert!(!needs_token(&Command::Delete { node: nid("a1") }));
        let stored = restore.clone();
        let j: Json = serde_json::from_str(r#"{"op":"batch","commands":[{"op":"restore","token":7}]}"#).unwrap();
        let c = command_from_json(&j, &|t| (t == 7).then(|| stored.clone())).unwrap();
        assert_eq!(c, Command::Batch(vec![restore]));
        let err = command_from_json(&j, &none).unwrap_err();
        assert_eq!((err.code, err.path.as_deref()), ("unknown_token", Some("commands[0].token")));
    }

    #[test]
    fn values_round_trip() {
        let vals = [
            Value::Bool(true),
            Value::Int(-4),
            Value::Float(0.25),
            Value::Str("a".into()),
            Value::Vec2([1.0, 2.5]),
            Value::Vec3([0.1, 0.2, 0.3]),
            Value::Vec4([0.0, 1.0, 2.0, 3.0]),
            Value::Color([1.0, 0.5, 0.25, 1.0]),
            Value::Enum { variant: "sphere".into(), fields: vec![("radius".into(), Value::Float(1.5))] },
            Value::unit("dynamic"),
            Value::List(vec![Value::Str("a".into()), Value::Int(1)]),
            Value::Map(vec![("a".into(), Value::Int(1)), ("b".into(), Value::None)]),
            Value::NodeRef("n1".into()),
            Value::Asset("a/b.png".into()),
            Value::None,
        ];
        for v in vals {
            let j = value_to_json(&v);
            let back = value_from_json(&j).unwrap();
            // maps are sorted by key in JSON; these are already sorted
            assert_eq!(back, v, "{j}");
        }
    }

    #[test]
    fn mistakes_name_the_place() {
        let bad = |s: &str| command_from_json(&serde_json::from_str(s).unwrap(), &none).unwrap_err();
        let e = bad(r#"{"op":"sett","node":"a","path":"x","value":1}"#);
        assert!(e.message.contains("did you mean set"), "{e}");
        let e = bad(r#"{"op":"batch","commands":[{"op":"delete","node":"UPPER"}]}"#);
        assert_eq!((e.code, e.path.as_deref()), ("invalid_node_id", Some("commands[0].node")));
        let e = bad(r#"{"op":"set","node":"a","path":"x"}"#);
        assert_eq!(e.path, None);
        assert!(e.message.contains("missing `value`"));
        let e = bad(r#"{"op":"spawn","node":{"components":{"bodyy":{}}}}"#);
        assert!(e.message.contains("did you mean body"), "{e}");
    }

    #[test]
    fn whole_numbers_can_be_widened() {
        let set = Command::SetProperty { node: nid("a"), path: "body.mass".into(), value: Value::Int(5) };
        let w = widen_ints(&set).unwrap();
        assert_eq!(w, Command::SetProperty { node: nid("a"), path: "body.mass".into(), value: Value::Float(5.0) });
        assert!(widen_ints(&w).is_none());
    }
}
