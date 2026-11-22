//! Scripts attached to objects (ADR 0005, section 4).
//!
//! An object with a `script` component (`script = "src/door.luau"` plus `props` in a scene file, or
//! `obj:attach_script(path, props)`) gets one **instance** of that script module:
//!
//! ```lua
//! local Door = {}
//! Door.props = { open_angle = 90, speed = 2 }      -- declared props and their defaults
//! function Door:start() self.closed = self.rotation end
//! function Door:update(dt) self.object:rotate_y(self.speed * dt) end
//! function Door:on_collision(other, info) end
//! return Door
//! ```
//!
//! * The module is loaded once per path (through `require`, so `lua.sandbox(true)`, the strict globals and hot reload
//!   apply) and cached. Every object gets its own `self` table: the props, `self.object` (the `Obj` handle) and
//!   whatever the script stores. Reads and writes of `Object` members on `self` (`self.position`, `self:rotate_y(a)`)
//!   forward to the object; any other key is instance state.
//! * Callbacks: `start`, `update`, `fixed_update`, `late_update`, `on_collision`, `on_destroy`. They run after the
//!   global callback of the same name, in the order the scripts were attached (a monotonic sequence number).
//! * Isolation: every callback is a guarded call. An error, a watchdog interrupt (infinite loop) or an allocation
//!   failure marks only that instance `Failed`: it is reported once (log, devtools, [`ScriptFailure`]) and skipped
//!   from then on, while the game and the other objects keep running.
//!
//! With no scripted object the host never gets here: `ScriptGame` checks two empty lists first.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};

use mlua::{Function, IntoLuaMulti, Lua, MultiValue, ObjectLike, Table, Value};
use spark_core::{ObjectId, ScriptComponent, ScriptFailure};
use spark_reflect::Value as Prop;

use crate::convert::*;
use crate::names;
use crate::types::{LuaColor, Obj};

pub(crate) type SharedObjects = Rc<RefCell<ObjectScripts>>;

/// Callbacks an object script may define.
pub const OBJECT_CALLBACKS: &[&str] = &["start", "update", "fixed_update", "late_update", "on_collision", "on_destroy"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    Running,
    Failed,
    Gone,
}

/// A loaded script module: its callbacks, declared props and the metatable of its instances.
struct Class {
    props: Vec<(String, Value)>,
    meta: Table,
    start: Option<Function>,
    update: Option<Function>,
    fixed_update: Option<Function>,
    late_update: Option<Function>,
    on_collision: Option<Function>,
    on_destroy: Option<Function>,
}

struct Instance {
    seq: u64,
    id: ObjectId,
    path: String,
    state: State,
    class: Option<Rc<Class>>,
    this: Option<Table>,
}

/// Which per-frame callback a stage runs.
#[derive(Clone, Copy)]
pub(crate) enum Stage {
    Fixed,
    Update,
    Late,
}

impl Stage {
    fn name(self) -> &'static str {
        match self {
            Stage::Fixed => "fixed_update",
            Stage::Update => "update",
            Stage::Late => "late_update",
        }
    }

    fn pick(self, c: &Class) -> Option<&Function> {
        match self {
            Stage::Fixed => c.fixed_update.as_ref(),
            Stage::Update => c.update.as_ref(),
            Stage::Late => c.late_update.as_ref(),
        }
    }
}

pub(crate) struct ObjectScripts {
    /// In attach order (increasing `seq`).
    instances: Vec<Instance>,
    by_object: HashMap<ObjectId, usize>,
    classes: HashMap<String, Result<Rc<Class>, String>>,
    last_epoch: u64,
    max_seq: u64,
    /// The host's watchdog: every callback gets its own full budget.
    deadline: Rc<Cell<Option<Instant>>>,
    timeout: Duration,
    strict: bool,
    failed: usize,
    members: Rc<HashSet<String>>,
}

impl ObjectScripts {
    pub(crate) fn new(deadline: Rc<Cell<Option<Instant>>>, timeout: Duration, strict: bool) -> Self {
        let members = crate::types::object_member_names().into_iter().map(str::to_string).collect();
        Self {
            instances: Vec::new(),
            by_object: HashMap::new(),
            classes: HashMap::new(),
            last_epoch: 0,
            max_seq: 0,
            deadline,
            timeout,
            strict,
            failed: 0,
            members: Rc::new(members),
        }
    }

    /// Drops every Lua reference (call before dropping the Lua state).
    pub(crate) fn clear(&mut self) {
        self.instances.clear();
        self.by_object.clear();
        self.classes.clear();
    }

    pub(crate) fn has_instances(&self) -> bool {
        !self.instances.is_empty()
    }

    /// Number of instances that failed and are skipped now.
    pub(crate) fn failed(&self) -> usize {
        self.failed
    }

    fn compact(&mut self) {
        self.instances.retain(|i| i.state != State::Gone);
        self.by_object = self.instances.iter().enumerate().map(|(n, i)| (i.id, n)).collect();
    }
}

fn shared(lua: &Lua) -> Option<SharedObjects> {
    lua.app_data_ref::<SharedObjects>().map(|s| s.clone())
}

// ---------------------------------------------------------------- loading a script

fn lua_kind(v: &Value) -> &'static str {
    match v {
        Value::Number(_) | Value::Integer(_) => "a number",
        Value::String(_) => "a string",
        Value::Boolean(_) => "a boolean",
        Value::Vector(_) => "a vector",
        Value::Table(_) => "a table",
        Value::UserData(_) => "a color",
        _ => "a value",
    }
}

/// Does the file/`attach_script` value fit the type of the declared default?
fn compatible(def: &Value, v: &Prop) -> bool {
    matches!(
        (def, v),
        (Value::Number(_) | Value::Integer(_), Prop::Int(_) | Prop::Float(_))
            | (Value::String(_), Prop::Str(_) | Prop::Asset(_) | Prop::NodeRef(_))
            | (Value::Boolean(_), Prop::Bool(_))
            | (Value::Vector(_), Prop::Vec3(_))
            | (Value::Table(_), Prop::List(_) | Prop::Map(_) | Prop::Vec2(_) | Prop::Vec4(_))
            | (Value::UserData(_), Prop::Color(_) | Prop::Vec4(_))
    )
}

/// A prop value from a document as a Luau value.
fn prop_to_lua(lua: &Lua, v: &Prop) -> LuaResult<Value> {
    Ok(match v {
        Prop::Bool(b) => Value::Boolean(*b),
        Prop::Int(i) => Value::Number(*i as f64),
        Prop::Float(f) => Value::Number(*f),
        Prop::Str(s) | Prop::Asset(s) | Prop::NodeRef(s) => Value::String(lua.create_string(s)?),
        Prop::Vec3([x, y, z]) => Value::Vector(mlua::Vector::new(*x, *y, *z)),
        Prop::Vec2([x, y]) => table_of(lua, &[("x", *x), ("y", *y)])?,
        Prop::Vec4([x, y, z, w]) => table_of(lua, &[("x", *x), ("y", *y), ("z", *z), ("w", *w)])?,
        Prop::Color([r, g, b, a]) => Value::UserData(lua.create_userdata(LuaColor(spark_core::Color::rgba(*r, *g, *b, *a)))?),
        Prop::List(items) => {
            let t = lua.create_table()?;
            for item in items {
                t.push(prop_to_lua(lua, item)?)?;
            }
            Value::Table(t)
        }
        Prop::Map(pairs) => {
            let t = lua.create_table()?;
            for (k, item) in pairs {
                t.set(k.as_str(), prop_to_lua(lua, item)?)?;
            }
            Value::Table(t)
        }
        Prop::Enum { variant, .. } => Value::String(lua.create_string(variant)?),
        Prop::None => Value::Nil,
    })
}

fn table_of(lua: &Lua, fields: &[(&str, f32)]) -> LuaResult<Value> {
    let t = lua.create_table()?;
    for (k, v) in fields {
        t.set(*k, *v)?;
    }
    Ok(Value::Table(t))
}

/// A Luau value as a prop value (for `obj:attach_script(path, props)`). Numbers with no fraction become integers.
pub(crate) fn lua_to_prop(v: &Value, what: &str, depth: usize) -> LuaResult<Prop> {
    if depth > 8 {
        return Err(rt(format!("{what}: nested too deeply (at most 8 levels of tables)")));
    }
    Ok(match v {
        Value::Boolean(b) => Prop::Bool(*b),
        Value::Integer(i) => Prop::Int(*i),
        Value::Number(_) => {
            let n = to_num64(v, what)?;
            if n.fract() == 0.0 && n.abs() < 9.0e15 { Prop::Int(n as i64) } else { Prop::Float(n) }
        }
        Value::String(s) => Prop::Str(s.to_string_lossy()),
        Value::Vector(x) => Prop::Vec3(to_vec3(&Value::Vector(*x), what)?.to_array()),
        Value::UserData(ud) => match ud.borrow::<LuaColor>() {
            Ok(c) => Prop::Color([c.0.r, c.0.g, c.0.b, c.0.a]),
            Err(_) => return Err(rt(format!("{what}: only numbers, strings, booleans, vectors, colors and tables can be props"))),
        },
        Value::Table(t) => {
            if t.raw_len() > 0 {
                let mut items = Vec::new();
                for (i, item) in t.clone().sequence_values::<Value>().enumerate() {
                    items.push(lua_to_prop(&item?, &format!("{what}[{}]", i + 1), depth + 1)?);
                }
                Prop::List(items)
            } else {
                let mut pairs = Vec::new();
                for pair in t.pairs::<Value, Value>() {
                    let (k, item) = pair?;
                    let Value::String(k) = k else { return Err(rt(format!("{what}: table keys must be strings"))) };
                    let k = k.to_string_lossy();
                    let p = lua_to_prop(&item, &format!("{what}.{k}"), depth + 1)?;
                    pairs.push((k, p));
                }
                pairs.sort_by(|a, b| a.0.cmp(&b.0));
                Prop::Map(pairs)
            }
        }
        other => return Err(rt(format!("{what}: a {} cannot be a prop", other.type_name()))),
    })
}

/// `Door.props = { ... }` values must not be shared between instances: tables are copied.
fn deep_copy(lua: &Lua, v: &Value, depth: usize) -> LuaResult<Value> {
    match v {
        Value::Table(t) if depth < 8 => {
            let c = lua.create_table()?;
            for pair in t.pairs::<Value, Value>() {
                let (k, item) = pair?;
                c.raw_set(k, deep_copy(lua, &item, depth + 1)?)?;
            }
            Ok(Value::Table(c))
        }
        other => Ok(other.clone()),
    }
}

/// Wraps a method of the `Obj` so that `self:rotate_y(a)` (with `self` the instance table) calls it on the object.
fn bound(lua: &Lua, f: Function, obj: mlua::AnyUserData) -> LuaResult<Value> {
    let w = lua.create_function(move |_, args: MultiValue| {
        let mut v: Vec<Value> = args.into_iter().collect();
        if v.is_empty() {
            v.push(Value::UserData(obj.clone()));
        } else {
            v[0] = Value::UserData(obj.clone());
        }
        f.call::<MultiValue>(MultiValue::from_vec(v))
    })?;
    Ok(Value::Function(w))
}

/// The metatable of a script's instances: methods of the module, then members of the `Object`.
fn make_meta(lua: &Lua, module: Table, members: Rc<HashSet<String>>) -> LuaResult<Table> {
    let meta = lua.create_table()?;
    let (m, mem) = (module, members.clone());
    meta.set(
        "__index",
        lua.create_function(move |lua, (this, key): (Table, Value)| -> LuaResult<Value> {
            let Value::String(name) = &key else { return Ok(Value::Nil) };
            let found: Value = m.get(key.clone())?;
            if !found.is_nil() {
                return Ok(found);
            }
            if !mem.contains(&*name.to_str()?) {
                return Ok(Value::Nil);
            }
            let Value::UserData(ud) = this.raw_get::<Value>("object")? else { return Ok(Value::Nil) };
            match ud.get::<Value>(key)? {
                Value::Function(f) => bound(lua, f, ud),
                v => Ok(v),
            }
        })?,
    )?;
    meta.set(
        "__newindex",
        lua.create_function(move |_, (this, key, value): (Table, Value, Value)| -> LuaResult<()> {
            if let Value::String(name) = &key {
                if members.contains(&*name.to_str()?) {
                    if let Value::UserData(ud) = this.raw_get::<Value>("object")? {
                        return ud.set(key, value);
                    }
                }
            }
            this.raw_set(key, value)
        })?,
    )?;
    Ok(meta)
}

fn load_class(lua: &Lua, members: &Rc<HashSet<String>>, path: &str) -> Result<Rc<Class>, String> {
    let clean = |e: mlua::Error| names::clean_error(&e.to_string());
    let value = crate::modules::require_game(lua, path).map_err(clean)?;
    let Value::Table(module) = value else {
        let got = match value {
            Value::Boolean(true) => "nothing".to_string(),
            other => other.type_name().to_string(),
        };
        return Err(format!("script {path} must return a table (the script), got {got}: write `local Script = {{}}` ... `return Script`"));
    };
    // Misspelled callbacks: `Update`, `on_colision`, `render` (object scripts have no render callback).
    let mut problems = Vec::new();
    for pair in module.pairs::<Value, Value>() {
        let (k, v) = pair.map_err(clean)?;
        if let (Value::String(k), Value::Function(f)) = (&k, &v) {
            let name = k.to_string_lossy();
            if let Some(msg) = names::object_callback_problem(&name) {
                let info = f.info();
                let at = match (info.short_src, info.line_defined) {
                    (Some(src), Some(line)) => format!("{src}:{line}: "),
                    _ => String::new(),
                };
                problems.push((name, format!("{at}{msg}")));
            }
        }
    }
    if !problems.is_empty() {
        problems.sort();
        return Err(problems.into_iter().map(|(_, m)| m).collect::<Vec<_>>().join("\n"));
    }
    // Declared props.
    let mut props: Vec<(String, Value)> = Vec::new();
    match module.get::<Value>("props").map_err(clean)? {
        Value::Nil => {}
        Value::Table(t) => {
            for pair in t.pairs::<Value, Value>() {
                let (k, v) = pair.map_err(clean)?;
                let Value::String(k) = k else {
                    return Err(format!("script prop of {path}: prop names must be strings, got a {} key", k.type_name()));
                };
                let k = k.to_string_lossy();
                if k == "object" || members.contains(&k) {
                    return Err(collision(path, &k));
                }
                if matches!(v, Value::Function(_) | Value::Thread(_) | Value::Nil) {
                    return Err(format!(
                        "script prop '{k}' of {path} has a {} default: props hold numbers, strings, booleans, vectors, colors or tables",
                        v.type_name()
                    ));
                }
                props.push((k, v));
            }
        }
        other => {
            return Err(format!(
                "script {path}: `props` must be a table of defaults, e.g. `Script.props = {{ speed = 2 }}`, got {}",
                other.type_name()
            ));
        }
    }
    props.sort_by(|a, b| a.0.cmp(&b.0));
    let f = |name: &str| module.get::<Option<Function>>(name).map_err(clean);
    let meta = make_meta(lua, module.clone(), members.clone()).map_err(clean)?;
    Ok(Rc::new(Class {
        props,
        meta,
        start: f("start")?,
        update: f("update")?,
        fixed_update: f("fixed_update")?,
        late_update: f("late_update")?,
        on_collision: f("on_collision")?,
        on_destroy: f("on_destroy")?,
    }))
}

fn collision(path: &str, name: &str) -> String {
    format!(
        "script prop '{name}' of {path} collides with the Object member obj.{name}: reading or writing self.{name} would reach the object, \
         not the prop. Rename it (e.g. 'door_{name}')"
    )
}

/// The instance table of `id` for a loaded class, with the props of the component applied.
fn make_this(
    lua: &Lua,
    members: &HashSet<String>,
    class: &Class,
    path: &str,
    id: ObjectId,
    script: &ScriptComponent,
) -> Result<Table, String> {
    let err = |e: mlua::Error| names::clean_error(&e.to_string());
    let this = lua.create_table().map_err(err)?;
    this.raw_set("object", Obj(id)).map_err(err)?;
    for (k, v) in &class.props {
        this.raw_set(k.as_str(), deep_copy(lua, v, 0).map_err(err)?).map_err(err)?;
    }
    for (k, v) in &script.props {
        if k == "object" || members.contains(k) {
            return Err(collision(path, k));
        }
        let Some((_, def)) = class.props.iter().find(|(n, _)| n == k) else {
            let declared: Vec<&str> = class.props.iter().map(|(n, _)| n.as_str()).collect();
            let did = names::suggest(k, declared.iter().copied()).map(|s| format!(" (did you mean '{s}'?)")).unwrap_or_default();
            let list = if declared.is_empty() {
                "it declares none: add `Script.props = { ... }`".to_string()
            } else {
                format!("declared props: {}", declared.join(", "))
            };
            return Err(format!("script prop '{k}' is not declared by {path}{did}; {list}"));
        };
        if !compatible(def, v) {
            return Err(format!("script prop '{k}' of {path} must be {} (like its default), got {}", lua_kind(def), v.type_name()));
        }
        this.raw_set(k.as_str(), prop_to_lua(lua, v).map_err(err)?).map_err(err)?;
    }
    this.set_metatable(Some(class.meta.clone())).map_err(err)?;
    Ok(this)
}

// ---------------------------------------------------------------- failures

/// Marks instance `n` failed and reports it once: the log (with the code), the world's failure list for devtools.
fn fail_instance(lua: &Lua, st: &SharedObjects, n: usize, what: &str, raw: &str) {
    let msg = names::clean_error(raw);
    let code = names::classify(&msg);
    let (path, id, strict) = {
        let mut s = st.borrow_mut();
        let i = &mut s.instances[n];
        let (path, id) = (i.path.clone(), i.id);
        if i.state == State::Running {
            i.state = State::Failed;
            s.failed += 1;
        }
        (path, id, s.strict)
    };
    let (name, node) = crate::convert::with(lua, |w| Ok((w.scene.get(id).map(|o| o.name.clone()), w.node_id(id)))).unwrap_or((None, None));
    let who = match (&name, &node) {
        (Some(n), Some(node)) if !n.is_empty() => format!("object \"{n}\" (node {node})"),
        (_, Some(node)) => format!("object (node {node})"),
        _ => format!("object #{} (already destroyed)", id.index()),
    };
    let full = format!("script {path}: error in {what} of {who}: {msg}");
    log::error!("[{code}] {full}");
    let failure =
        ScriptFailure { node_id: node, object: name.unwrap_or_default(), code: code.to_string(), message: format!("[{code}] {full}") };
    let _ = crate::convert::with(lua, |w| {
        w.report_script_failure(failure);
        Ok(())
    });
    if strict {
        std::process::exit(1);
    }
}

/// Calls one callback of instance `n` under the watchdog; a failure fails only this instance.
fn call_cb(lua: &Lua, st: &SharedObjects, n: usize, cb: &str, f: Function, args: impl IntoLuaMulti) {
    let (deadline, timeout) = {
        let s = st.borrow();
        (s.deadline.clone(), s.timeout)
    };
    crate::LOAD_TIME.with(|c| c.set(Duration::ZERO));
    deadline.set(Instant::now().checked_add(timeout));
    let result = f.call::<()>(args);
    // A tripped watchdog leaves its deadline in the past: the rest of the frame needs a fresh one.
    deadline.set(Instant::now().checked_add(timeout));
    if let Err(e) = result {
        fail_instance(lua, st, n, &format!("{cb}()"), &e.to_string());
    }
}

// ---------------------------------------------------------------- the frame

/// Attaches the scripts that appeared and retires the ones that went away (calling `on_destroy`), in spawn order.
/// Cheap when nothing changed: one comparison of the scene's script epoch.
pub(crate) fn sync(lua: &Lua) -> LuaResult<()> {
    let Some(st) = shared(lua) else { return Ok(()) };
    // A `start` may attach more scripts: go on a few rounds, the rest waits for the next frame.
    for _ in 0..16 {
        let epoch = with(lua, |w| Ok(w.scene.script_epoch()))?;
        if epoch == st.borrow().last_epoch {
            return Ok(());
        }
        let entries = with(lua, |w| Ok(w.scene.scripted().to_vec()))?;
        st.borrow_mut().last_epoch = epoch;
        let gone: Vec<usize> = {
            let s = st.borrow();
            s.instances
                .iter()
                .enumerate()
                .filter(|(_, i)| i.state != State::Gone)
                .filter(|(_, i)| entries.binary_search_by_key(&i.seq, |e| e.0).map_or(true, |k| entries[k].1 != i.id))
                .map(|(n, _)| n)
                .collect()
        };
        for n in gone {
            retire(lua, &st, n);
        }
        let max = st.borrow().max_seq;
        for (seq, id) in entries.into_iter().filter(|e| e.0 > max) {
            st.borrow_mut().max_seq = seq;
            attach(lua, &st, seq, id)?;
        }
        st.borrow_mut().compact();
    }
    Ok(())
}

/// Creates the instance of object `id` and runs its `start`.
fn attach(lua: &Lua, st: &SharedObjects, seq: u64, id: ObjectId) -> LuaResult<()> {
    let Some(script) = with(lua, |w| Ok(w.scene.get(id).and_then(|o| o.script().cloned())))? else { return Ok(()) };
    let path = script.path.clone();
    let members = st.borrow().members.clone();
    let class = {
        let cached = st.borrow().classes.get(&path).cloned();
        match cached {
            Some(c) => c,
            None => {
                let c = load_class(lua, &members, &path);
                st.borrow_mut().classes.insert(path.clone(), c.clone());
                c
            }
        }
    };
    let built = class.and_then(|class| make_this(lua, &members, &class, &path, id, &script).map(|this| (class, this)));
    let n = {
        let mut s = st.borrow_mut();
        s.instances.push(Instance { seq, id, path, state: State::Running, class: None, this: None });
        s.instances.len() - 1
    };
    st.borrow_mut().by_object.insert(id, n);
    match built {
        Err(e) => {
            fail_instance(lua, st, n, "loading", &e);
        }
        Ok((class, this)) => {
            {
                let mut s = st.borrow_mut();
                s.instances[n].class = Some(class.clone());
                s.instances[n].this = Some(this.clone());
            }
            if let Some(f) = &class.start {
                call_cb(lua, st, n, "start", f.clone(), this);
            }
        }
    }
    Ok(())
}

/// The script of instance `n` is gone (object destroyed, script detached or replaced): `on_destroy`, then forget it.
fn retire(lua: &Lua, st: &SharedObjects, n: usize) {
    let call = {
        let mut s = st.borrow_mut();
        let i = &mut s.instances[n];
        let was = i.state;
        i.state = State::Gone;
        let call = (was == State::Running).then(|| i.class.as_ref().and_then(|c| c.on_destroy.clone()).zip(i.this.clone())).flatten();
        if was == State::Failed {
            s.failed = s.failed.saturating_sub(1);
        }
        call
    };
    if let Some((f, this)) = call {
        call_cb(lua, st, n, "on_destroy", f, this);
    }
}

/// Runs the `fixed_update` / `update` / `late_update` of every running instance, in spawn order.
pub(crate) fn run_stage(lua: &Lua, stage: Stage, dt: f32) -> LuaResult<()> {
    let Some(st) = shared(lua) else { return Ok(()) };
    let mut n = 0;
    loop {
        let call = {
            let s = st.borrow();
            let Some(i) = s.instances.get(n) else { break };
            (i.state == State::Running).then(|| i.class.as_ref().and_then(|c| stage.pick(c)).cloned().zip(i.this.clone())).flatten()
        };
        if let Some((f, this)) = call {
            call_cb(lua, &st, n, stage.name(), f, (this, dt));
        }
        n += 1;
    }
    Ok(())
}

/// `on_collision(self, other, info)` of the object that collided (called for the start of a contact).
pub(crate) fn on_collision(lua: &Lua, me: ObjectId, other: ObjectId, info: &Table) -> LuaResult<()> {
    let Some(st) = shared(lua) else { return Ok(()) };
    let call = {
        let s = st.borrow();
        if s.instances.is_empty() {
            return Ok(());
        }
        s.by_object.get(&me).copied().and_then(|n| {
            let i = s.instances.get(n)?;
            (i.state == State::Running && i.id == me).then_some(())?;
            Some((n, i.class.as_ref()?.on_collision.clone()?, i.this.clone()?))
        })
    };
    if let Some((n, f, this)) = call {
        call_cb(lua, &st, n, "on_collision", f, (this, Obj(other), info.clone()));
    }
    Ok(())
}

/// `obj:destroy()`: the scripts of the object and its descendants get `on_destroy` while the object still exists.
pub(crate) fn before_destroy(lua: &Lua, id: ObjectId) -> LuaResult<()> {
    let Some(st) = shared(lua) else { return Ok(()) };
    if st.borrow().instances.is_empty() {
        return Ok(());
    }
    let subtree: HashSet<ObjectId> = with(lua, |w| {
        let mut set = HashSet::new();
        let mut stack = vec![id];
        while let Some(o) = stack.pop() {
            if w.scene.contains(o) && set.insert(o) {
                stack.extend(w.scene.children(o));
            }
        }
        Ok(set)
    })?;
    let hits: Vec<usize> = {
        let s = st.borrow();
        s.instances.iter().enumerate().filter(|(_, i)| i.state != State::Gone && subtree.contains(&i.id)).map(|(n, _)| n).collect()
    };
    for n in hits {
        retire(lua, &st, n);
    }
    Ok(())
}

/// What `obj.script` and the failure list show: the script path of an object.
pub(crate) fn script_path(lua: &Lua, id: ObjectId) -> LuaResult<Option<String>> {
    with(lua, |w| Ok(w.scene.get(id).ok_or_else(dead)?.script().map(|s| s.path.clone())))
}

/// `obj:attach_script(path, props?)`.
pub(crate) fn attach_script(lua: &Lua, id: ObjectId, path: String, props: Option<Table>) -> LuaResult<()> {
    if path.trim().is_empty() {
        return Err(rt("attach_script: the script path is empty (e.g. \"src/door.luau\")"));
    }
    let mut list = Vec::new();
    if let Some(t) = props {
        for pair in t.pairs::<Value, Value>() {
            let (k, v) = pair?;
            let Value::String(k) = k else { return Err(rt("attach_script: prop names must be strings")) };
            let k = k.to_string_lossy();
            let p = lua_to_prop(&v, &format!("attach_script prop '{k}'"), 0)?;
            list.push((k, p));
        }
        list.sort_by(|a, b| a.0.cmp(&b.0));
    }
    with(lua, |w| if w.scene.set_script(id, Some(ScriptComponent { path, props: list })) { Ok(()) } else { Err(dead()) })
}

/// `obj:detach_script()`: the script gets `on_destroy` at the next sync point and stops.
pub(crate) fn detach_script(lua: &Lua, id: ObjectId) -> LuaResult<()> {
    with(lua, |w| if w.scene.set_script(id, None) { Ok(()) } else { Err(dead()) })
}
