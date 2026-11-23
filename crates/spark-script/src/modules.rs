//! `require`: a game split into files.
//!
//! ```lua
//! local player = require("./src/player")     -- next to this file (.luau added, or src/player/init.luau)
//! local hud = require("../ui/hud")           -- one folder up
//! local config = require("@game/config")     -- from the game folder (where main.luau is)
//! ```
//! A module runs once and its return value is cached; every file is watched for hot reload.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::SystemTime;

use mlua::{ErrorContext, Lua, Table, Value};

use crate::convert::{LuaResult, rt};

pub(crate) struct Modules {
    /// Game folder (`@game/`).
    pub root: PathBuf,
    /// Every loaded module file with its modification time at load (hot reload).
    pub files: Vec<(PathBuf, Option<SystemTime>)>,
    /// Modules being loaded right now (cycle detection).
    loading: Vec<String>,
}

pub(crate) type SharedModules = Rc<RefCell<Modules>>;

pub(crate) fn new(root: PathBuf) -> SharedModules {
    Rc::new(RefCell::new(Modules { root, files: Vec::new(), loading: Vec::new() }))
}

const CACHE: &str = "spark.modules";

pub(crate) fn install(lua: &Lua, g: &Table, modules: SharedModules) -> LuaResult<()> {
    lua.set_named_registry_value(CACHE, lua.create_table()?)?;
    lua.set_app_data(modules);
    g.set("require", lua.create_function(require)?)?;
    Ok(())
}

/// Game-relative file of the Luau code that called `require` (chunk names are `@relative/path.luau`).
fn caller(lua: &Lua) -> String {
    for level in 0..10 {
        let src = lua.inspect_stack(level, |d| d.source().source.map(|s| s.into_owned()));
        match src {
            None => break,
            Some(Some(s)) => {
                if let Some(rel) = s.strip_prefix('@') {
                    return rel.replace('\\', "/");
                }
            }
            Some(None) => {}
        }
    }
    "main.luau".into()
}

/// `spec` as a game-relative path ('/' separated), without the extension.
fn resolve_spec(from: &str, spec: &str) -> Result<String, String> {
    let (base, rest) = if let Some(r) = spec.strip_prefix("@game/") {
        (String::new(), r)
    } else if spec.starts_with("./") || spec.starts_with("../") {
        (from.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default(), spec)
    } else {
        let hint = spec.trim_start_matches('/').trim_end_matches(".luau");
        return Err(format!(
            "require(\"{spec}\"): a path starts with \"./\" (next to this file), \"../\" (folder up) or \"@game/\" \
             (the game folder), e.g. require(\"./{hint}\")"
        ));
    };
    let mut parts: Vec<&str> = base.split('/').filter(|p| !p.is_empty()).collect();
    for p in rest.split(['/', '\\']) {
        match p {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(format!("require(\"{spec}\"): goes above the game folder"));
                }
            }
            p if p.contains([':', '\0']) => {
                return Err(format!("require(\"{spec}\"): drive letters and ':' are not allowed; modules live in the game folder"));
            }
            p => parts.push(p),
        }
    }
    if parts.is_empty() {
        return Err(format!("require(\"{spec}\"): needs a file name"));
    }
    Ok(parts.join("/"))
}

fn find_file(root: &std::path::Path, rel: &str) -> Result<String, Vec<String>> {
    let candidates: Vec<String> = if rel.ends_with(".luau") || rel.ends_with(".lua") {
        vec![rel.to_string()]
    } else {
        vec![format!("{rel}.luau"), format!("{rel}.lua"), format!("{rel}/init.luau")]
    };
    for c in &candidates {
        if spark_core::vfs::is_file(&root.join(c)) {
            return Ok(c.clone());
        }
    }
    Err(candidates)
}

/// Loads (or takes from the cache) the module at a game-relative path, like `require("@game/<path>")`. Used for the
/// scripts attached to objects, so they share the cache, the sandbox and hot reload with `require`.
pub(crate) fn require_game(lua: &Lua, path: &str) -> LuaResult<Value> {
    require(lua, format!("@game/{path}"))
}

/// Game-relative file (`src/door.luau`) that `require_game(lua, path)` loads, or `None` when there is no such file.
pub(crate) fn game_file(lua: &Lua, path: &str) -> Option<String> {
    let modules = lua.app_data_ref::<SharedModules>()?.clone();
    let root = modules.borrow().root.clone();
    let rel = resolve_spec("main.luau", &format!("@game/{path}")).ok()?;
    find_file(&root, &rel).ok()
}

fn require(lua: &Lua, spec: String) -> LuaResult<Value> {
    let from = caller(lua);
    let modules = lua.app_data_ref::<SharedModules>().ok_or_else(|| rt("require is not available here"))?.clone();
    let root = modules.borrow().root.clone();
    let rel = resolve_spec(&from, &spec).map_err(rt)?;
    let rel = find_file(&root, &rel)
        .map_err(|tried| rt(format!("require(\"{spec}\") from {from}: no such module (looked for {})", tried.join(", "))))?;
    let cache: Table = lua.named_registry_value(CACHE)?;
    let cached: Value = cache.raw_get(rel.as_str())?;
    if !cached.is_nil() {
        return Ok(cached);
    }
    {
        let m = modules.borrow();
        if let Some(i) = m.loading.iter().position(|x| *x == rel) {
            let mut chain: Vec<&str> = m.loading[i..].iter().map(String::as_str).collect();
            chain.push(&rel);
            return Err(rt(format!(
                "require cycle: {} - modules can't require each other while loading; move the shared part into \
                 a third module, or call require inside a function",
                chain.join(" -> ")
            )));
        }
    }
    let path = root.join(&rel);
    let source = spark_core::vfs::read_string(&path).map_err(rt)?;
    // Watched before it runs: saving a fix for a broken module reloads the game.
    let mtime = spark_core::vfs::modified(&path);
    modules.borrow_mut().files.push((path, mtime));
    let value = run_module(lua, &modules, &rel, &source)?;
    cache.raw_set(rel.as_str(), value.clone())?;
    Ok(value)
}

/// Runs the module's file once and returns what it returned (`true` for a module that returns nothing).
fn run_module(lua: &Lua, modules: &SharedModules, rel: &str, source: &str) -> LuaResult<Value> {
    modules.borrow_mut().loading.push(rel.to_string());
    let result = crate::chunk(lua, source).set_name(format!("@{rel}")).into_function().and_then(|f| f.call::<Value>(()));
    modules.borrow_mut().loading.pop();
    Ok(match result? {
        Value::Nil => Value::Boolean(true),
        v => v,
    })
}

/// Files of loaded modules that changed on disk since they were (re)loaded.
pub(crate) fn changed(modules: &SharedModules) -> Vec<PathBuf> {
    let m = modules.borrow();
    m.files.iter().filter(|(p, t)| t.is_some() && spark_core::vfs::modified(p) != *t).map(|(p, _)| p.clone()).collect()
}

/// What [`reload`] did.
pub(crate) enum Reload {
    /// The module table was updated in place: everybody who holds it sees the new code.
    InPlace,
    /// Not possible without losing track of the old value (the module returns a function, a string, ...): restart the game.
    NeedsRestart(String),
}

/// Hot reload of one module (ADR 0006): runs the file again and, when old and new value are tables, copies the new
/// fields into the table everybody already holds (fields that are gone are removed, the metatable follows), so
/// `local Player = require("./player")` in other modules and the classes of object scripts see the new functions. Then
/// the module's optional `on_reload(old)` is called with a copy of the table as it was before (`old.counter` ...).
///
/// The file's modification time is recorded first: a broken save is reported once, not on every poll.
pub(crate) fn reload(lua: &Lua, path: &std::path::Path) -> LuaResult<Reload> {
    let modules = lua.app_data_ref::<SharedModules>().ok_or_else(|| rt("require is not available here"))?.clone();
    let root = modules.borrow().root.clone();
    let rel = path.strip_prefix(&root).unwrap_or(path).to_string_lossy().replace('\\', "/");
    let cache: Table = lua.named_registry_value(CACHE)?;
    let old: Value = cache.raw_get(rel.as_str())?;
    let mtime = spark_core::vfs::modified(path);
    if let Some(e) = modules.borrow_mut().files.iter_mut().find(|(p, _)| p == path) {
        e.1 = mtime;
    }
    if old.is_nil() {
        return Ok(Reload::NeedsRestart(format!("{rel} did not finish loading before")));
    }
    let source = spark_core::vfs::read_string(path).map_err(rt)?;
    let new = run_module(lua, &modules, &rel, &source)?;
    match (&old, &new) {
        (Value::Table(o), Value::Table(n)) => {
            if o.to_pointer() == n.to_pointer() {
                return Ok(Reload::InPlace);
            }
            if o.is_readonly() {
                return Ok(Reload::NeedsRestart(format!("{rel} returns a frozen table")));
            }
            let snapshot = lua.create_table()?;
            let mut stale = Vec::new();
            for pair in o.pairs::<Value, Value>() {
                let (k, v) = pair?;
                if n.raw_get::<Value>(k.clone())?.is_nil() {
                    stale.push(k.clone());
                }
                snapshot.raw_set(k, v)?;
            }
            snapshot.set_metatable(o.metatable())?;
            for k in stale {
                o.raw_set(k, Value::Nil)?;
            }
            let fields: Vec<(Value, Value)> = n.pairs::<Value, Value>().collect::<LuaResult<_>>()?;
            let n_meta = n.metatable();
            for (k, v) in fields {
                // `Player.__index = Player` points at the new table: make it the one everybody holds.
                let own = matches!(&v, Value::Table(t) if t.to_pointer() == n.to_pointer());
                o.raw_set(k.clone(), if own { Value::Table(o.clone()) } else { v })?;
                // The new table keeps only its metamethods (`__index = Player` written in the module, so that
                // `setmetatable(obj, Player)` inside the new code works) and forwards everything else to the held
                // table. So code of this version that reads or writes `Player.count`, or looks up a method for an
                // object it created, always sees the held table, also after the next reload.
                if !matches!(&k, Value::String(s) if s.as_bytes().starts_with(b"__")) {
                    n.raw_set(k, Value::Nil)?;
                }
            }
            let proxy = lua.create_table()?;
            proxy.raw_set("__index", o.clone())?;
            proxy.raw_set("__newindex", o.clone())?;
            n.set_metatable(Some(proxy))?;
            o.set_metatable(n_meta)?;
            if let Some(hook) = o.raw_get::<Option<mlua::Function>>("on_reload")? {
                hook.call::<()>(snapshot).map_err(|e| e.context(format!("in on_reload() of {rel}")))?;
            }
            Ok(Reload::InPlace)
        }
        (Value::Boolean(_), Value::Boolean(_)) => Ok(Reload::InPlace),
        _ => Ok(Reload::NeedsRestart(format!("{rel} returns a different kind of value than before"))),
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_spec;

    #[test]
    fn specs() {
        assert_eq!(resolve_spec("main.luau", "./src/player").unwrap(), "src/player");
        assert_eq!(resolve_spec("src/player.luau", "./input").unwrap(), "src/input");
        assert_eq!(resolve_spec("src/ui/hud.luau", "../world").unwrap(), "src/world");
        assert_eq!(resolve_spec("src/ui/hud.luau", "@game/config").unwrap(), "config");
        assert!(resolve_spec("main.luau", "player").unwrap_err().contains("require(\"./player\")"));
        assert!(resolve_spec("main.luau", "../x").is_err());
        // A drive prefix would make PathBuf::join replace the game folder on Windows.
        assert!(resolve_spec("main.luau", "./C:/Windows/win").is_err());
        assert!(resolve_spec("main.luau", "@game/C:x").is_err());
        assert!(resolve_spec("main.luau", "./a\\..\\..\\x").is_err());
    }
}
