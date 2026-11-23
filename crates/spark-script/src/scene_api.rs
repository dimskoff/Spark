//! `scene.load`, `scene.save` and `spawn("x.prefab", props)`: levels and prefabs (docs/SCENE_FORMAT.md, ADR 0003).
//!
//! ```lua
//! local roots = scene.load("levels/arena.scene", { replace = true })
//! local door = spawn("prefabs/door.prefab", { position = vec3(4, 0, -2) })
//! scene.save("levels/arena.scene")
//! ```
//!
//! Paths are game-relative and stay inside the game folder, like every other file API. Reading works from a packed
//! (shipped) game; writing does not: `scene.save` is for games run from their folder.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use mlua::{Lua, Table, Value};
use spark_core::doc::SceneFile;
use spark_scene::{FsLoader, PrefabSource, load_scene, save_scene, spawn_prefab};

use crate::convert::*;
use crate::types::{Obj, apply_props};

/// True for the first argument of `spawn` that names a prefab file.
pub(crate) fn is_prefab_path(s: &str) -> bool {
    s.to_ascii_lowercase().ends_with(".prefab")
}

/// Parsed prefab files of this game, re-read when the file on disk changes.
#[derive(Default)]
struct PrefabFiles {
    root: PathBuf,
    files: HashMap<String, (Option<SystemTime>, SceneFile)>,
}

impl PrefabSource for PrefabFiles {
    fn prefab(&mut self, path: &str) -> Result<SceneFile, String> {
        let loader = FsLoader::new(&self.root, true);
        let stamp = loader.resolve(path).ok().and_then(|p| spark_core::vfs::modified(&p));
        if let Some((seen, file)) = self.files.get(path) {
            if *seen == stamp {
                return Ok(file.clone());
            }
        }
        let file = loader.read(path)?;
        self.files.insert(path.to_string(), (stamp, file.clone()));
        Ok(file)
    }
}

/// Runs `f` with the prefab files of the game folder of `world`.
fn with_files<R>(lua: &Lua, f: impl FnOnce(&mut spark_core::World, &mut PrefabFiles) -> LuaResult<R>) -> LuaResult<R> {
    if lua.app_data_ref::<RefCell<PrefabFiles>>().is_none() {
        lua.set_app_data(RefCell::new(PrefabFiles::default()));
    }
    let cell = lua.app_data_ref::<RefCell<PrefabFiles>>().ok_or_else(|| rt("scene: prefab cache is not available"))?;
    let mut files = cell.try_borrow_mut().map_err(|_| rt("scene: prefab files are busy (nested call)"))?;
    with(lua, |w| {
        files.root = w.assets.root.clone();
        f(w, &mut files)
    })
}

fn want_scene(what: &str, path: &str) -> LuaResult<()> {
    if path.to_ascii_lowercase().ends_with(".scene") {
        return Ok(());
    }
    let hint = if is_prefab_path(path) {
        if what == "scene.load" { " (a prefab is created with spawn(\"x.prefab\"))" } else { " (prefabs are written by the editor)" }
    } else {
        ""
    };
    Err(rt(format!("{what}: '{path}' must be a .scene file{hint}")))
}

fn load_opts(opts: Option<Table>) -> LuaResult<bool> {
    let mut replace = false;
    let Some(t) = opts else { return Ok(replace) };
    for pair in t.pairs::<String, Value>() {
        let (k, v) = pair?;
        match k.as_str() {
            "replace" => replace = to_bool(&v, "scene.load option 'replace'")?,
            _ => return Err(rt(format!("scene.load: unknown option '{k}' (allowed: replace)"))),
        }
    }
    Ok(replace)
}

/// `spawn("x.prefab", props?)`: `second` is the props table (the material slot of `spawn` has no meaning for a prefab).
pub(crate) fn spawn_prefab_call(lua: &Lua, path: &str, second: Value, third: Option<Table>) -> LuaResult<Obj> {
    let props = match (second, third) {
        (Value::Nil, props) => props,
        (Value::Table(t), None) => Some(t),
        (other, _) => {
            return Err(rt(format!(
                "spawn(\"{path}\", props): a prefab takes a table of properties as its second argument, got {}",
                other.type_name()
            )));
        }
    };
    with_files(lua, |w, files| {
        let (root, warnings) = spawn_prefab(w, path, files).map_err(|e| rt(format!("spawn(\"{path}\"): {e}")))?;
        for m in warnings {
            log::warn!("spawn(\"{path}\"): {m}");
        }
        if let Err(e) = apply_props(w, root, props) {
            w.scene.destroy(root);
            return Err(e);
        }
        Ok(Obj(root))
    })
}

/// Writes `text` to `path` through a temp file and a rename, so a crash leaves the old file or the new one.
fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create '{}': {e}", dir.display()))?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let err = |e: std::io::Error| format!("cannot write '{}': {e}", tmp.display());
    let mut f = std::fs::File::create(&tmp).map_err(err)?;
    f.write_all(text.as_bytes()).map_err(err)?;
    f.sync_all().map_err(err)?;
    drop(f);
    std::fs::rename(&tmp, path).map_err(|e| format!("cannot write '{}': {e}", path.display()))
}

pub(crate) fn install(lua: &Lua, g: &Table) -> LuaResult<()> {
    let scene = lua.create_table()?;
    scene.set(
        "load",
        lua.create_function(|lua, (path, opts): (String, Option<Table>)| {
            want_scene("scene.load", &path)?;
            let replace = load_opts(opts)?;
            let loaded = with_files(lua, |w, files| {
                let file = FsLoader::new(&files.root, true).read(&path).map_err(|e| rt(format!("scene.load: {e}")))?;
                load_scene(w, &file, files, replace).map_err(|e| rt(format!("scene.load(\"{path}\"): {e}")))
            })?;
            for m in &loaded.warnings {
                log::warn!("scene.load(\"{path}\"): {m}");
            }
            Ok(loaded.roots.into_iter().map(Obj).collect::<Vec<_>>())
        })?,
    )?;
    scene.set(
        "save",
        lua.create_function(|lua, path: String| {
            want_scene("scene.save", &path)?;
            if spark_core::vfs::is_packed() {
                return Err(rt(format!(
                    "scene.save(\"{path}\"): a packed game (spark build) cannot write its own files; scene.save works when the game runs from its folder"
                )));
            }
            let saved = with_files(lua, |w, files| {
                let full = w.assets.resolve(&path).map_err(|e| rt(format!("scene.save: {e}")))?;
                let saved = save_scene(w, files).map_err(|e| rt(format!("scene.save(\"{path}\"): {e}")))?;
                write_atomic(&full, &saved.text).map_err(|e| rt(format!("scene.save(\"{path}\"): {e}")))?;
                Ok(saved)
            })?;
            for m in &saved.warnings {
                log::warn!("scene.save(\"{path}\"): {m}");
            }
            Ok(saved.warnings)
        })?,
    )?;
    g.set("scene", scene)?;
    Ok(())
}
