//! `save` global: tiny persistent key/value storage (JSON file in the user's data folder).
//!
//! ```lua
//! save.set("best", 42)
//! local best = save.get("best", 0)
//! ```

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use mlua::{Lua, Table, Value};
use serde_json::{Map, Number, Value as Json};

use crate::convert::*;

/// Where the save file lives (`None` = in memory only, e.g. headless runs). Set by `ScriptGame`.
#[derive(Clone, Default)]
pub(crate) struct SavePath(pub Option<PathBuf>);

struct Store {
    path: Option<PathBuf>,
    data: RefCell<Map<String, Json>>,
    /// Changed since the last write. `save.set` in a loop costs one write per frame, not per call.
    dirty: std::cell::Cell<bool>,
    /// After a failed end-of-frame write: do not try again before this time.
    retry_at: std::cell::Cell<Option<std::time::Instant>>,
}

impl Drop for Store {
    fn drop(&mut self) {
        if let Err(e) = write_file(self) {
            log::error!("{e}");
        }
    }
}

/// `%APPDATA%/SparkGames/<game>/save.json` (Windows), `~/Library/Application Support/SparkGames/<game>/save.json`
/// (macOS) or `$XDG_DATA_HOME` / `~/.local/share/SparkGames/<game>/save.json` (Linux).
pub fn default_save_path(game: &str) -> Option<PathBuf> {
    spark_core::paths::game_dir(game).map(|d| d.join("save.json"))
}

fn load(path: &Path) -> Map<String, Json> {
    let Ok(text) = std::fs::read_to_string(path) else { return Map::new() };
    match serde_json::from_str::<Json>(&text) {
        Ok(Json::Object(m)) => m,
        _ => {
            // Keep the damaged file for recovery instead of overwriting it with the next save.
            let bak = path.with_extension("json.bak");
            let _ = std::fs::rename(path, &bak);
            log::warn!("save file '{}' is damaged; moved to '{}', starting fresh", path.display(), bak.display());
            Map::new()
        }
    }
}

/// Marks the store changed; the file is written by [`flush`] (end of frame) or on exit.
fn persist(store: &Store) -> LuaResult<()> {
    store.dirty.set(true);
    Ok(())
}

/// `save.flush()`: writes the save file now if something changed. A failed write is an error here.
pub(crate) fn flush(lua: &Lua) -> LuaResult<()> {
    match lua.app_data_ref::<Store>() {
        Some(s) => write_file(&s).map_err(rt),
        None => Ok(()),
    }
}

/// End of frame: writes the save file if something changed. A failed write (disk full, file locked by an
/// antivirus...) does not stop the game: it is logged and retried after [`RETRY`], the data stays in memory.
pub(crate) fn flush_frame(lua: &Lua) {
    let Some(s) = lua.app_data_ref::<Store>() else { return };
    if s.retry_at.get().is_some_and(|t| std::time::Instant::now() < t) {
        return;
    }
    match write_file(&s) {
        Ok(()) => s.retry_at.set(None),
        Err(e) => {
            log::error!("{e} (retrying in {} s; the data is kept in memory)", RETRY.as_secs());
            s.retry_at.set(Some(std::time::Instant::now() + RETRY));
        }
    }
}

const RETRY: std::time::Duration = std::time::Duration::from_secs(2);

/// Writes the file if dirty. Atomic and durable: temp file, fsync, rename (then fsync of the folder on Unix),
/// so a crash or power loss leaves either the old or the new save, never a torn one. Stays dirty on failure.
fn write_file(store: &Store) -> Result<(), String> {
    if !store.dirty.get() {
        return Ok(());
    }
    let Some(path) = &store.path else {
        store.dirty.set(false);
        return Ok(());
    };
    let text = serde_json::to_string_pretty(&*store.data.borrow()).map_err(|e| e.to_string())?;
    write_atomic(path, text.as_bytes())?;
    store.dirty.set(false);
    Ok(())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("save: cannot create '{}': {e}", dir.display()))?;
    }
    let tmp = path.with_extension("json.tmp");
    let err = |e: std::io::Error| format!("save: cannot write '{}': {e}", tmp.display());
    let mut f = std::fs::File::create(&tmp).map_err(err)?;
    f.write_all(bytes).map_err(err)?;
    f.sync_all().map_err(err)?;
    drop(f);
    std::fs::rename(&tmp, path).map_err(|e| format!("save: cannot write '{}': {e}", path.display()))?;
    #[cfg(unix)]
    if let Some(dir) = path.parent() {
        // Makes the rename itself durable.
        let _ = std::fs::File::open(dir).and_then(|d| d.sync_all());
    }
    Ok(())
}

/// Most values one `save.set` may store (a table that shares sub-tables would otherwise blow up).
const MAX_VALUES: usize = 100_000;

/// Converts a Luau value; `path` holds the tables being converted (cycle detection), `budget` counts values.
fn to_json(v: &Value, what: &str) -> LuaResult<Json> {
    to_json_inner(v, what, &mut Vec::new(), &mut 0)
}

fn to_json_inner(v: &Value, what: &str, path: &mut Vec<*const std::ffi::c_void>, budget: &mut usize) -> LuaResult<Json> {
    *budget += 1;
    if *budget > MAX_VALUES {
        return Err(rt(format!("{what}: too much data (more than {MAX_VALUES} values); save less, or store ids instead of copies")));
    }
    if path.len() > 64 {
        return Err(rt(format!("{what}: tables are nested more than 64 levels deep")));
    }
    Ok(match v {
        Value::Nil => Json::Null,
        Value::Boolean(b) => Json::Bool(*b),
        Value::Integer(i) => Json::Number((*i).into()),
        Value::Number(n) => {
            if n.fract() == 0.0 && n.abs() < 9.0e15 {
                Json::Number((*n as i64).into())
            } else {
                Json::Number(Number::from_f64(*n).ok_or_else(|| rt(format!("{what}: cannot save NaN / infinity")))?)
            }
        }
        Value::String(s) => Json::String(s.to_string_lossy()),
        Value::Vector(v) => {
            let mut m = Map::new();
            m.insert("$vec".into(), Json::Array(vec![v.x().into(), v.y().into(), v.z().into()]));
            Json::Object(m)
        }
        Value::Table(t) => {
            let ptr = t.to_pointer();
            if path.contains(&ptr) {
                return Err(rt(format!("{what}: a table contains itself (cycle); save cannot store cycles")));
            }
            path.push(ptr);
            let r = table_to_json(t, what, path, budget);
            path.pop();
            r?
        }
        other => {
            return Err(rt(format!(
                "{what}: cannot save a {} (only numbers, strings, booleans, vectors and tables of them)",
                other.type_name()
            )));
        }
    })
}

fn table_to_json(t: &Table, what: &str, path: &mut Vec<*const std::ffi::c_void>, budget: &mut usize) -> LuaResult<Json> {
    let len = t.raw_len();
    let mut count = 0usize;
    for pair in t.clone().pairs::<Value, Value>() {
        pair?;
        count += 1;
    }
    if len > 0 && count == len {
        let mut a = Vec::with_capacity(len);
        for i in 1..=len {
            a.push(to_json_inner(&t.raw_get::<Value>(i)?, what, path, budget)?);
        }
        return Ok(Json::Array(a));
    }
    let mut m = Map::new();
    for pair in t.clone().pairs::<Value, Value>() {
        let (k, val) = pair?;
        let key = match &k {
            Value::String(s) => s.to_string_lossy(),
            Value::Integer(i) => i.to_string(),
            Value::Number(n) => n.to_string(),
            _ => return Err(rt(format!("{what}: table keys must be strings or numbers, got {}", k.type_name()))),
        };
        m.insert(key, to_json_inner(&val, what, path, budget)?);
    }
    Ok(Json::Object(m))
}

fn to_lua(lua: &Lua, j: &Json) -> LuaResult<Value> {
    Ok(match j {
        Json::Null => Value::Nil,
        Json::Bool(b) => Value::Boolean(*b),
        Json::Number(n) => match n.as_i64() {
            Some(i) if i.abs() < (1 << 53) => Value::Number(i as f64),
            _ => Value::Number(n.as_f64().unwrap_or(0.0)),
        },
        Json::String(s) => Value::String(lua.create_string(s)?),
        Json::Array(a) => {
            let t = lua.create_table_with_capacity(a.len(), 0)?;
            for (i, v) in a.iter().enumerate() {
                t.raw_set(i + 1, to_lua(lua, v)?)?;
            }
            Value::Table(t)
        }
        Json::Object(m) => {
            if let (1, Some(Json::Array(v))) = (m.len(), m.get("$vec")) {
                let c = |i: usize| v.get(i).and_then(Json::as_f64).unwrap_or(0.0) as f32;
                return Ok(Value::Vector(mlua::Vector::new(c(0), c(1), c(2))));
            }
            let t = lua.create_table()?;
            for (k, v) in m {
                t.raw_set(k.as_str(), to_lua(lua, v)?)?;
            }
            Value::Table(t)
        }
    })
}

fn store(lua: &Lua) -> LuaResult<mlua::AppDataRef<'_, Store>> {
    lua.app_data_ref::<Store>().ok_or_else(|| rt("save: storage is not available"))
}

pub(crate) fn install(lua: &Lua, g: &Table) -> LuaResult<()> {
    let path = lua.app_data_ref::<SavePath>().map(|p| p.0.clone()).unwrap_or_default();
    let data = path.as_deref().map(load).unwrap_or_default();
    lua.set_app_data(Store { path, data: RefCell::new(data), dirty: Default::default(), retry_at: Default::default() });

    let save = lua.create_table()?;
    save.set(
        "get",
        lua.create_function(|lua, (key, default): (String, Value)| {
            let s = store(lua)?;
            let v = s.data.borrow().get(&key).cloned();
            match v {
                Some(j) if !j.is_null() => to_lua(lua, &j),
                _ => Ok(default),
            }
        })?,
    )?;
    save.set(
        "set",
        lua.create_function(|lua, (key, value): (String, Value)| {
            let j = to_json(&value, &format!("save.set(\"{key}\")"))?;
            let s = store(lua)?;
            if j.is_null() {
                s.data.borrow_mut().remove(&key);
            } else {
                s.data.borrow_mut().insert(key, j);
            }
            persist(&s)
        })?,
    )?;
    save.set("flush", lua.create_function(|lua, ()| flush(lua))?)?;
    save.set("has", lua.create_function(|lua, key: String| Ok(store(lua)?.data.borrow().contains_key(&key)))?)?;
    save.set(
        "delete",
        lua.create_function(|lua, key: String| {
            let s = store(lua)?;
            let had = s.data.borrow_mut().remove(&key).is_some();
            persist(&s)?;
            Ok(had)
        })?,
    )?;
    save.set(
        "clear",
        lua.create_function(|lua, ()| {
            let s = store(lua)?;
            s.data.borrow_mut().clear();
            persist(&s)
        })?,
    )?;
    save.set("keys", lua.create_function(|lua, ()| Ok(store(lua)?.data.borrow().keys().cloned().collect::<Vec<_>>()))?)?;
    save.set("path", lua.create_function(|lua, ()| Ok(store(lua)?.path.as_ref().map(|p| p.display().to_string())))?)?;
    g.set("save", save)?;
    Ok(())
}
