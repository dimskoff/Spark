# ADR 0006: hot reload of one module; `spark mcp` scene and live-game tools

Status: accepted (Stage C, step C-18). Builds on ADR 0002 (sandbox), ADR 0005 (commands, object scripts) and devtools protocol 4. Roadmap: Stage C rows "hot reload of a module without restarting the game" and "`spark mcp`: scene_read, scene_apply, attach".

## 1. Hot reload

**Which change does what.** The host checks the watched files every 50 ms (`POLL_EVERY`; a few `stat` calls).
- `main.luau` changed, or any module changed while the host is stopped on an error: full restart, as before (`World::reset`, new Lua state, `start()`).
- Only modules changed: `reload_modules`. The world, the objects, physics bodies and every loaded asset stay.
- A module that cannot be swapped in place (it returns a function or a string, a frozen table, or never finished loading) falls back to the full restart.

**Swap in place.** The file runs again through the same sandboxed `chunk()` as the first `require`, so it sees the current globals (the dead-code prefix of ADR 0002 makes global reads dynamic; nothing is baked). When old and new value are tables, the new fields are copied into the table everybody already holds; fields that disappeared are removed; `__index = Player` is re-pointed at the held table. The new table keeps only its `__*` metamethods and forwards every other read and write to the held table (`__index`/`__newindex`), so code of this version, and objects built with `setmetatable(o, Player)` in any earlier version, always reach the latest functions and module state after any number of reloads. Locals inside the module start fresh (they belong to the new chunk).

**`on_reload(old)`.** If the module table has `on_reload`, it is called once after the swap, as a plain function, with a shallow copy of the table as it was before (`old.count`). It is the place to carry state over (`Player.count = old.count`). Errors in it are reported like load errors.

**Errors.** A module that fails (syntax or runtime) keeps its previous version; the problem is shown as a "Module error" banner and reported through `Game::error` to devtools. Its modification time is recorded first, so the error is shown once; saving a fix reloads it. The banner is cleared by the next successful reload or restart.

**Object scripts.** The class of every script whose file changed is rebuilt. Running instances **keep their `self` table** (props and stored state) and get the new metatable and callbacks, plus defaults of props that are new. No `on_destroy` and no `start`: the object did not go away, only its code changed. An instance that had failed (or never loaded) is recreated with the props of its component and its `start` runs, so saving a fix gives the object another try. A script that no longer loads keeps its old class.

**Assets.** A module reload does not touch assets or the scene. A `main.luau` restart still clears them (`Assets::clear` invalidates ids, the renderer drops its GPU copies by generation, and procedural meshes share the id space with model meshes, so a partial keep needs id remapping). Not done in C-18; the model load on restart is the cost to measure (about 11 ms for 50 small models here), and the module path does not pay it.

**Stale handles.** The history of undo commands lives in the editor; in the game the stale-prone state is the table of `restore` tokens in the devtools command host. `World::reset` bumps a counter (`World::resets`); the host drops its tokens when it changes. Module reloads keep node ids valid, so tokens stay.

**Measured.** 50 models (generated GLB rigs, each loaded separately): full `start()` 11 ms; module reload 0.2 to 0.7 ms after the change is noticed (test `saving_a_module_with_50_models_loaded_takes_under_100_ms`), plus up to 50 ms of polling.

## 2. MCP tools

All run inside the `spark mcp` process (`crates/spark-cli/src/mcp_scene.rs`); the older tools still run `spark <cmd> --json` children.

| Tool | Input | Output |
|---|---|---|
| `scene_read` | `path?` | file: `{ok, mode:"file", path, nodes, text}`; no path, attached: `{ok, mode:"live", scene:<SceneSnapshot>}` (the devtools snapshot, not a `.scene`: protocol 4 has no scene-file message and none was added) |
| `scene_apply` | `commands[]`, `path?`, `root?`, `dry_run?` | `{ok:true, mode, changed[], inverse, resolved?, written?, warnings?}` or `{ok:false, error:{code, message, path, suggestions[]}}` |
| `attach` | `path?` (launch) or `listen:true`, `timeout_ms?` | connected: `{ok, state, protocol, engine, pid, gpu, ...}`; listen: `{ok, state:"waiting", addr, token}` |
| `game_control` | `action` (status, pause, resume, step, reload, overlay, stats), `frames?`, `on?` | `{ok, connected, paused, script_error, stats, ...}` |
| `detach` | none | `{ok, was_attached, graceful}` |

- **Commands** are the JSON form of `spark_devtools::command_json`. The call is wrapped in one `batch`, so it is all-or-nothing. Parse errors carry a path such as `commands[1].op`; apply errors carry the reflect path and did-you-mean suggestions.
- **Inverse.** `inverse` is one command that undoes the whole call: send it back as the only entry of `commands`. Offline, inverses use JSON where possible (including the `spawn` that undoes a `delete`); exact states with no JSON form (`restore`) become `{"op":"restore","token":N}`, kept in the MCP process (256 most recent), so they are valid for the life of the server. Live, the game hands out its own tokens.
- **Offline apply**: parse the file, `load_scene` into a headless `World`, `World::apply`, `save_scene`, atomic write (temp file + rename). Nothing is written on any error. The result is the engine's canonical text: a non-canonical file is rewritten in canonical form on its first apply.
- **Attach.** The editor listens and the game connects, so MCP owns a `Server` with a fresh token; the token goes to the game through `SPARK_DEVTOOLS_TOKEN` (never argv). Launch mode starts `spark <game> --devtools <addr>` (a window, needs a GPU); listen mode lets an existing launcher start the game. One session at a time. No screenshot message exists in the protocol, so the attached game cannot be screenshotted; use `spark_screenshot` for headless pictures.

## Limitations
- `restore` tokens (offline) are per MCP process, and exact only for edits that do not hold mesh or texture handles; JSON `set` inverses cover those.
- Live scene reads are the snapshot (objects, camera), not component values.
- A `main.luau` save still reloads assets.
