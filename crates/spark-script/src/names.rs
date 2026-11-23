//! "Zero silent errors": typo detection (did-you-mean), error classification and traceback cleanup.

use std::collections::HashSet;

use mlua::{Lua, Table, Value};

/// Functions the engine calls by global name.
pub const CALLBACKS: &[&str] = &["start", "update", "fixed_update", "late_update", "render"];

/// Events the engine emits itself (`on` / `once` / `obj:on`). Anything far from these is a custom event.
pub const BUILTIN_EVENTS: &[&str] =
    &["key_pressed", "key_released", "mouse_pressed", "mouse_released", "wheel", "collision", "collision_end"];

/// Other spellings of a callback that other engines use.
const CALLBACK_ALIASES: &[(&str, &str)] = &[
    ("draw2d", "render"),
    ("draw_2d", "render"),
    ("ondraw", "render"),
    ("onrender", "render"),
    ("onupdate", "update"),
    ("onstart", "start"),
    ("physicsupdate", "fixed_update"),
];

/// Levenshtein distance (with adjacent transpositions counting 1).
pub fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}

/// Closest of `candidates` to `name`: equal ignoring case / `_`, or within an edit distance that grows with the name length.
pub fn suggest<'a>(name: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let squash = |s: &str| s.to_lowercase().replace('_', "");
    let n = squash(name);
    let max = if n.chars().count() <= 4 { 1 } else { 2 };
    let mut best: Option<(&str, usize)> = None;
    for c in candidates {
        if c == name {
            continue;
        }
        let d = if squash(c) == n { 0 } else { distance(&name.to_lowercase(), &c.to_lowercase()) };
        if d <= max && best.is_none_or(|(_, bd)| d < bd) {
            best = Some((c, d));
        }
    }
    best.map(|(c, _)| c)
}

/// The callback `name` was probably meant to be, if it is a function that looks like a misspelled callback
/// (`Update`, `fixedUpdate`, `draw2d`, `updaet`). Helpers like `updateScore` are not near enough.
pub fn misspelled_callback(name: &str) -> Option<&'static str> {
    misspelled_in(name, CALLBACKS, true)
}

/// Same for the callbacks of an object script (`on_colision`, `Update`, `onDestroy`).
pub fn misspelled_object_callback(name: &str) -> Option<&'static str> {
    misspelled_in(name, crate::object_scripts::OBJECT_CALLBACKS, false)
}

/// What is wrong with a function named `name` in an object script module, if it looks like a mistake.
pub(crate) fn object_callback_problem(name: &str) -> Option<String> {
    let list = crate::object_scripts::OBJECT_CALLBACKS.join(", ");
    if name == "render" {
        return Some(format!(
            "'render' is not an object script callback: 2D drawing belongs in the global render() of main.luau. Object script callbacks: {list}"
        ));
    }
    if crate::object_scripts::OBJECT_CALLBACKS.contains(&name) {
        return None;
    }
    misspelled_object_callback(name).map(|right| {
        format!("'{name}' is not a Spark callback (did you mean '{right}'?) - object script callbacks are lowercase snake_case: {list}")
    })
}

fn misspelled_in(name: &str, set: &[&'static str], aliases: bool) -> Option<&'static str> {
    if set.contains(&name) {
        return None;
    }
    let n = name.to_lowercase().replace('_', "");
    if aliases {
        if let Some((_, right)) = CALLBACK_ALIASES.iter().find(|(a, _)| *a == n) {
            return Some(right);
        }
    }
    for c in set.iter().copied() {
        let cn = c.replace('_', "");
        if n == cn {
            return Some(c);
        }
        // One typo: same length (substitution / swap) or one letter missing; long names only ("star" or "stars" are fine helper names).
        if cn.len() >= 6 {
            let d = distance(&n, &cn);
            if d == 1 && n.len() <= cn.len() {
                return Some(c);
            }
        } else if n.len() == cn.len()
            && distance(&n, &cn) == 1
            && n.chars().next() == cn.chars().next()
            && n.chars().last() == cn.chars().last()
        {
            // "strat" for "start": same ends, swapped / wrong letters inside
            return Some(c);
        }
    }
    None
}

pub fn callback_error(name: &str, right: &str) -> String {
    format!(
        "'{name}' is not a Spark callback (did you mean '{right}'?) - callbacks are lowercase snake_case: \
         start, update, fixed_update, late_update, render"
    )
}

/// Error for `on("colision", ...)` when `name` is a near miss of a built-in event.
pub fn event_error(what: &str, name: &str) -> Option<String> {
    if BUILTIN_EVENTS.contains(&name) {
        return None;
    }
    let max = |c: &&str| if c.len() <= 5 { 1 } else { 2 };
    let right =
        BUILTIN_EVENTS.iter().filter(|c| distance(&name.to_lowercase(), c) <= max(c)).min_by_key(|c| distance(&name.to_lowercase(), c))?;
    Some(format!(
        "{what}: '{name}' is not a built-in event (did you mean '{right}'?). Built-in events: {}. \
         Custom events may have any other name",
        BUILTIN_EVENTS.join(", ")
    ))
}

/// Message for reading / writing a field that does not exist on a Spark object.
pub fn field_error(write: bool, ty: &str, name: &str, known: &[&str]) -> String {
    let verb = if write { "attempt to set an unknown field" } else { "attempt to read an unknown field" };
    match suggest(name, known.iter().copied()) {
        Some(s) => format!("{verb} '{name}' on {ty} (did you mean '{s}'?)"),
        None => format!("{verb} '{name}' on {ty}"),
    }
}

/// Makes reading a global that was never assigned an error ("unknown global 'mth' (did you mean 'math'?)").
/// Writing new globals stays allowed, `rawget(_G, name)` still returns nil. Engine code must use `raw_get`.
///
/// Call it on the BASE globals (Spark's libraries) before `lua.sandbox(true)`: the sandbox's per-script
/// environment falls back to the base table, and a miss there lands in this `__index`. The suggestions
/// are taken from the base table and from the script's own globals (`lua.globals()` after the sandbox).
pub(crate) fn strict_globals(lua: &Lua) -> mlua::Result<()> {
    let mt = lua.create_table()?;
    mt.set(
        "__index",
        lua.create_function(|lua, (globals, key): (Table, Value)| -> mlua::Result<Value> {
            let Value::String(name) = key else { return Ok(Value::Nil) };
            let name = name.to_string_lossy().to_string();
            let mut keys: Vec<String> = globals.pairs::<String, Value>().filter_map(|p| p.ok().map(|(k, _)| k)).collect();
            keys.extend(lua.globals().pairs::<String, Value>().filter_map(|p| p.ok().map(|(k, _)| k)));
            Err(mlua::Error::runtime(match suggest(&name, keys.iter().map(String::as_str)) {
                Some(s) => format!("unknown global '{name}' (did you mean '{s}'?)"),
                None => format!("unknown global '{name}' (never assigned: declare it with `local {name}` or assign it before reading)"),
            }))
        })?,
    )?;
    lua.globals().set_metatable(Some(mt))?;
    Ok(())
}

/// After `main.luau` ran: a new global function that is a near miss of a callback is a typo, not a helper.
pub(crate) fn check_callback_names(lua: &Lua, before: &HashSet<String>) -> mlua::Result<()> {
    let mut found: Vec<(String, String)> = Vec::new();
    for pair in lua.globals().pairs::<String, Value>() {
        let Ok((name, Value::Function(f))) = pair else { continue };
        if before.contains(&name) {
            continue;
        }
        if let Some(right) = misspelled_callback(&name) {
            let info = f.info();
            let at = match (info.short_src, info.line_defined) {
                (Some(src), Some(line)) => format!("{src}:{line}: "),
                _ => String::new(),
            };
            found.push((name.clone(), format!("{at}{}", callback_error(&name, right))));
        }
    }
    found.sort();
    if found.is_empty() {
        return Ok(());
    }
    Err(mlua::Error::runtime(found.into_iter().map(|(_, m)| m).collect::<Vec<_>>().join("\n")))
}

/// Removes engine-internal lines from an error / traceback: `__mlua_*` chunks and bare `[C]: in ?` frames.
/// The user's `file:line` frames stay.
pub fn clean_error(msg: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for line in msg.lines() {
        let t = line.trim();
        if t == "[C]: in ?" || t.contains("__mlua_") {
            continue;
        }
        out.push(line);
    }
    while out.last().is_some_and(|l| l.trim() == "stack traceback:" || l.trim().is_empty()) {
        out.pop();
    }
    out.join("\n").replace(READONLY, READONLY_EXPLAINED)
}

/// Luau's message for a write into a table that `lua.sandbox` made read-only.
const READONLY: &str = "attempt to modify a readonly table";
const READONLY_EXPLAINED: &str = "cannot modify a built-in library (Spark's libraries and API tables - math, string, table, Mesh, \
     input, ... - are read-only; keep your own data in your own table or local variable, e.g. `local M = {}`)";

/// Stable error code of a script error message (see "Error codes" in docs/SPARK_API.md).
pub fn classify(message: &str) -> &'static str {
    let m = message;
    if m.contains("unknown global") {
        "SPK-1001"
    } else if m.contains("is not a Spark callback") {
        "SPK-1002"
    } else if m.contains("is not a built-in event") {
        "SPK-1003"
    } else if m.contains("unknown field") {
        "SPK-1004"
    } else if m.contains("without returning (infinite loop") {
        "SPK-1005"
    } else if m.contains("was destroyed") || m.contains("destroyed object") {
        "SPK-1006"
    } else if m.contains("cannot modify a built-in library") || m.contains(READONLY) {
        "SPK-1007"
    } else if m.contains("script prop '") {
        "SPK-1008"
    } else {
        "SPK-1000"
    }
}

/// `did you mean 'x'?` part of a message as a standalone hint.
pub fn hint(message: &str) -> Option<String> {
    let at = message.find("did you mean '")?;
    let rest = &message[at + "did you mean '".len()..];
    let end = rest.find('\'')?;
    Some(format!("use '{}'", &rest[..end]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_names() {
        for (w, r) in [
            ("Update", "update"),
            ("fixedUpdate", "fixed_update"),
            ("lateupdate", "late_update"),
            ("Render", "render"),
            ("Start", "start"),
            ("strat", "start"),
        ] {
            assert_eq!(misspelled_callback(w), Some(r), "{w}");
        }
        for ok in ["update", "start", "updateScore", "render_ui", "stars", "star", "helper", "renderer", "restart", "updates", "makeUpdate"]
        {
            assert_eq!(misspelled_callback(ok), None, "{ok}");
        }
    }

    #[test]
    fn suggestions() {
        assert_eq!(suggest("mth", ["math", "string", "mesh"]), Some("math"));
        assert_eq!(suggest("Vec3", ["vec3", "vec2"]), Some("vec3"));
        assert_eq!(suggest("zzzzz", ["math", "string"]), None);
        assert_eq!(suggest("postion", ["position", "rotation"]), Some("position"));
    }

    #[test]
    fn events() {
        assert!(event_error("on", "colision").unwrap().contains("did you mean 'collision'?"));
        assert!(event_error("on", "player_died").is_none());
        assert!(event_error("on", "collision").is_none());
        assert!(event_error("on", "when").is_none());
    }

    #[test]
    fn cleaning() {
        let m = "runtime error: main.luau:2: boom\nstack traceback:\n\t[C]: in ?\n\t[C]: in ?\n\t__mlua_newindex:16: in function <__mlua_newindex:4>\n\tmain.luau:2: in function update";
        let c = clean_error(m);
        assert_eq!(c, "runtime error: main.luau:2: boom\nstack traceback:\n\tmain.luau:2: in function update");
        assert_eq!(clean_error("x\nstack traceback:\n\t[C]: in ?"), "x");
    }

    #[test]
    fn codes() {
        assert_eq!(classify("unknown global 'mth' (did you mean 'math'?)"), "SPK-1001");
        assert_eq!(classify("'Update' is not a Spark callback (did you mean 'update'?)"), "SPK-1002");
        assert_eq!(classify("on: 'x' is not a built-in event (did you mean 'y'?)"), "SPK-1003");
        assert_eq!(classify("attempt to set an unknown field 'a' on Object"), "SPK-1004");
        assert_eq!(classify("script ran for more than 5.0 s without returning (infinite loop?)"), "SPK-1005");
        assert_eq!(classify("object was destroyed"), "SPK-1006");
        assert_eq!(classify("attempt to modify a readonly table"), "SPK-1007");
        assert_eq!(classify(&clean_error("x: attempt to modify a readonly table")), "SPK-1007");
        assert_eq!(classify("boom"), "SPK-1000");
        assert_eq!(hint("unknown global 'mth' (did you mean 'math'?)").as_deref(), Some("use 'math'"));
        assert_eq!(hint("boom"), None);
    }
}
