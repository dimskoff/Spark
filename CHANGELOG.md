# Changelog

Spark is in 0.x: every minor version may change the Luau API. After `spark update` run `spark docs .` in your game,
read the entries below for your old version and the new one, fix the code, then raise `spark = "..."` in `game.toml`.
`spark check` reports a version mismatch under `notes`.

## Unreleased

### Behavior changes (stricter validation; check your game if it relied on the old leniency)
- **Numbers must be finite.** `NaN`, `inf` and values that overflow a float now raise an error naming the argument
  (before: `every(0/0, fn)` fired 1000 times per frame, `Mesh.cube(0/0)` made a broken mesh, `camera.fov = 0/0` blanked the screen).
  Applies to every numeric argument, vectors and colors.
- **File paths from game code stay inside the game folder.** `Texture.load`, `Model.load`, `Font.load`, `sound.play`, `Shader.load`,
  `sky { cubemap = ... }` and `require` reject absolute paths, drive letters (`C:`), UNC paths, `..` above the game folder and
  `:` (before: any file on the machine could be opened). glTF / FBX texture lookup rejects drive prefixes too.
- Unknown `--flags` when running a game are an error with a "did you mean" hint (before: ignored, a window opened).
- `obj:every(0, fn)` is an error like the global `every(0, fn)`.
- **Reading an undefined global is an error** with a suggestion (`unknown global 'mth' (did you mean 'math'?)`). Writing new
  globals and `rawget(_G, name)` still work; `if some_unassigned_global then` now fails - declare it with `local` first.
- **A misspelled callback is an error** (`function Update`, `fixedUpdate`, `Render`, `draw2d` -> "did you mean 'update'?")
  instead of never being called. Helper names like `updateScore` are not affected.
- `on` / `once` / `obj:on` / `obj:once` reject a near miss of a built-in event (`colision` -> "did you mean 'collision'?").
  Custom event names still work.
- **Luau sandbox:** built-in libraries (`math`, `string`, `table`, ...) and Spark API tables (`Mesh`, `Material`, `input`, `ui`,
  `draw`, `sound`, ...) are read-only; writing to them (`math.sin = nil`, `Mesh.cube = f`, `setmetatable(string, ...)`) fails
  with `SPK-1007`. Scripts get their own writable globals shared by `main.luau`, modules, tasks and tests; `_G` and callbacks
  work as before; your own globals are always read with their current value in every file (main, modules, tests).
  Side effect: `math.*`-heavy scripts run about 3x faster (Luau safe-env fast paths for built-ins).
- `--devtools` accepts only loopback addresses and is ignored by packed (shipped) games.
- An unknown key in a `spawn{...}` table suggests the closest one (`colour` -> "did you mean 'color'?"); the list of allowed
  keys in that message is in a new order.
- **Unknown object fields are errors for reads too** (`cube.postion` -> "did you mean 'position'?"); before, reading one
  returned `nil`.

### Fixes
- Materials with alpha < 0.3 and blending (alpha / additive) are no longer cut out (the 0.3 cutout applies to opaque materials only).
- `save`: a failed write no longer loses the "unsaved" mark; the file is written with fsync + atomic rename, a failure while
  playing is logged and retried every 2 s instead of stopping the game; real cycle detection (the old "cycle?" appeared on any
  table nested deeper than 32 levels); a 100k-value limit per `save.set`.
- Game packs: entry sizes are validated against the file before allocating; files over 1 GiB are refused.
- Devtools: lines over 8 MiB cut the connection; the incoming queue is bounded.
- `spark check` accepts UTF-8 files with a BOM (it reported a syntax error on every file).
- Spark Editor: untagged stderr lines (panics) are no longer shown as Info.

- Script error tracebacks no longer show engine-internal frames (`__mlua_*`, `[C]: in ?`).
- **Security / robustness:**
  - FBX loader no longer panics or exhausts memory on malformed files (checked indices and arithmetic, caps on nesting,
    nodes, arrays and inflated bytes, parent cycles repaired); glTF index buffers are validated and a panic inside the
    `gltf` crate becomes a load error. Seeded mutation tests in `spark-core/tests/fbx_robustness.rs`, cargo-fuzz skeleton in `fuzz/`.
  - Devtools link is authenticated (protocol 3): Spark Editor issues a per-run token through `SPARK_DEVTOOLS_TOKEN` and drops
    connections with a wrong or missing token or from non-loopback peers.
  - `ImageData` is validated (size cap 16384, checked arithmetic); invalid images fall back to a placeholder instead of panicking.
  - `spark build` does not follow symlinks / junctions and passes `--` to `tar`; the exe-icon step uses a unique temp file.
  - Installers verify the new binary before replacing the old one; `install.ps1` rolls back on failure, `install.sh` uses
    `mktemp` + atomic `mv` and honours `SPARK_ALLOW_UNVERIFIED`.
  - The renderer no longer mutates the environment (`unsafe remove_var("WGPU_BACKEND")` removed); `#![forbid(unsafe_code)]`
    in every crate without FFI, the rest (`spark`, `spark-cli`, `spark-editor`) deny it outside the Win32 modules.
- Spark Editor survives a panic of the in-editor game (viewport stops, message in console / problems / toast; script
  watchdog 3 s) and its Scene inspector can edit a running game's object (position, rotation, scale, visibility).
- The infinite-loop message points to `task(function() ... end)` instead of the non-existent `task.spawn`.

### Tooling
- Devtools protocol 4: `EditorCommand::Command` applies a JSON `Command` (spawn, delete, set, reparent, add/remove component, batch) to the running game through `World::apply` and answers with the inverse command, the changed node ids or a structured error (`code`, `message`, `path`, did-you-mean `suggestions`). Scene snapshots carry `node_id`. `SetObject` still works (now a batch of `SetProperty`); games and editors of protocol 2 and 3 still connect. See ADR 0005, C-13.
- CI runs `cargo fmt --check` and `clippy -D warnings` with no allowed lints; `rustfmt.toml` added.
- Every `spark check --json` error has a stable `code` (`SPK-1000`..`SPK-1006`, `SPK-1100` syntax, `SPK-1101` types) and an
  optional `hint`; the table is under "Error codes" in `docs/SPARK_API.md`.
- **Third-party notices are shipped**: `THIRD_PARTY_LICENSES.md` (generated by `cargo run -p licenses` from `cargo metadata`,
  with a staleness test; a compact list of name / versions / license / repository / copyright holders plus one copy of each
  license text, ~100 KB) goes into release archives; `spark build` writes `THIRD_PARTY_LICENSES.txt` next to the game and into
  its archive; `spark licenses` and `game --licenses` print it.
- `deny.toml` + a CI `cargo-deny` job (licenses, bans, sources). `ASSETS.md` lists the origin and license of every media file.
- The stray `crates/spark-editor.zip` archive was removed; `*.zip` is ignored.
- Removed foreign assets that came from another project (`figure.glb` test model, `hit.wav` / `spawn.wav` demo sounds). The
  model tests generate a small skinned rig at run time in a temp folder; the demo example no longer plays sounds.
- `--seed N` seeds `math.random` for reproducible runs; headless runs, `spark check` and `spark test` default to seed 0
  (reseeded on restart and hot reload); windowed runs without `--seed` stay random. `spark check` / `spark test` accept `--seed N`.
- Packed games write `error.log` and `spark-crash.log` to `<user data>/SparkGames/<game>/logs/` (they could not be written
  next to the exe or inside the pack before); dev runs still write `error.log` next to `main.luau`. The error screen and the
  crash dialog show the real path. `spark_core::paths` holds the shared per-user folders.
### Added
- **`.scene` format v1** (TOML, ADR 0003, described in `docs/SCENE_FORMAT.md`): the new `spark-scene` crate reads and writes scene and prefab files; the document model, `capture(world)` and `instantiate(doc, world)` are in `spark_core::doc`. The writer is canonical (field order, defaults left out, degrees, hex colors, shortest numbers), so loading and saving a canonical file gives the same bytes. Errors carry the line, unknown component and field names come with a "did you mean", duplicate ids and missing parents are rejected, a mesh made by `Mesh.custom` or a texture handle is reported by node instead of being dropped.
- **Prefabs and scenes in scripts**: `scene.load(path, { replace = bool })` returns the root objects of a `.scene`, `scene.save(path)` writes every object and the environment to a `.scene` (returns a list of warnings; not in packed games), and `spawn("prefabs/door.prefab", { position = ... })` creates an instance of a `.prefab`. Paths stay inside the game folder; errors name the file and the line. A scene node with `prefab = "..."` is an instance: its own components replace the prefab root's, `[node.overrides]` (`"leaf/material.color" = "#8a3b2f"`) changes the prefab's other nodes, and children added in the scene use `parent = "door1"` or `parent = "door1/leaf"`. When saving, overrides are computed by diffing the instance against a fresh one, so a value set back to the prefab's disappears from the file. v1: property overrides and added children only; nested prefabs are rejected, removed or reparented prefab nodes are reported as warnings and not saved. New in the Rust API: `spark_scene::{load_scene, save_scene, spawn_prefab, expand, collapse}`, `spark_core::doc::instantiate_replacing`. New example `examples/08-scenes`.
- **Object scripts** (ADR 0005): `obj:attach_script("src/door.luau", { speed = 4 })`, `obj.script`, or `[node.script]` with `[node.script.props]` in a `.scene` / `.prefab` (instantiating a scene now attaches scripts instead of warning). The module returns a table with `props` (defaults) and `start / update / fixed_update / late_update / on_collision / on_destroy`; each object gets a `self` with `self.object`, forwarded Object members (`self.position`, `self:rotate_y(a)`) and its own state. Callbacks run after the global ones, in attach order. An error or infinite loop in one object's script fails only that object (logged once with `SPK-xxxx`, script, callback, object name and node id; devtools `script_error` gets an optional `node_id`); the game and the other objects keep running. Unknown or mistyped props and props named like an Object member are errors with a "did you mean" (`SPK-1008`); misspelled object callbacks are `SPK-1002`. See "Object scripts" in `docs/SPARK_API.md`.
- **`spark migrate [--check] [--json] <path>...`**: rewrites `.scene` / `.prefab` files (or a folder of them) to the current `format_version`; `--check` writes nothing and exits 1 when a file needs migration. A file with a newer version than your Spark is refused with "update Spark".
- **API registry**: `docs/API_REFERENCE.md`, `docs/api.json`, `llms.txt` are generated from `docs/spark.d.luau`; tests fail when the docs and the engine disagree. `spark new` ships them plus `CLAUDE.md`.
- **Agent CLI**: `spark api search|show|list`, `spark doctor`, `spark screenshot`, `spark inspect`, `spark golden`, `spark diff`, `spark mcp` (Model Context Protocol server on stdio); `--json` on `new`, `docs`, `build`, `update`. Exit codes: 0 ok, 1 failed, 2 usage/environment.
- Range checks: `camera.fov` 1..179, `near`/`far`, mesh sizes and radii, body `mass`/`friction`.
- Devtools protocol 2: `SetObject` (edit a running object), `Hello.token`. Luau memory limit (1 GiB).
- 12 examples (`examples/01-hello` ... `showcase/`) with golden images; the old `starfall` example was removed.
- **FBX import** (`.fbx` in `Model.load` / `spawn`): binary and ASCII 7.x, hierarchy, meshes per material, skins, animation stacks,
  unit / up-axis conversion, texture lookup next to the file with automatic PBR sets (`_Normal`, `_AO`, `_Roughness`, `_Metallic`, `_Opacity`).
- `spawn(model, { node = "name" })` and `model.nodes`: use one part of a model that holds a whole kit.
- **ORM maps** (`mat:with_orm_map`, `obj.orm_map`): ambient occlusion / roughness / metallic from one texture in PBR materials.
- Opaque materials now cut out pixels with alpha < 0.3 (was 0.01): foliage no longer shows dark fringes.
### Luau API / tools
- `spark new --template 3d|2d|empty` (the flag was documented but did nothing). New games get `tests/smoke.luau`.
- `spark check` also validates every `.wgsl`, `.png` / `.jpg` and `.gltf` / `.glb` in the game, and takes `--types`
  (type-check with luau-lsp). The JSON report has a new `notes` array.
- `spark test`: `test.texts()`, `test.sees(s)`, `test.expect_text(s)`, `test.wait_text(s)` read what the game drew.
- Docs inside games and in this repo now share one name: `docs/SPARK_API.md`, `docs/SPARK_SHADERS.md`
  (this repo used `LUAU_API.md` / `SHADERS.md`).

### Fixes
- The sky gradient / cubemap was mirrored vertically (reversed-Z introduced in the renderer was not applied to the
  skybox ray). Games that worked around it by swapping `top` / `bottom` colors should undo that.

### Packaging
- Release archives contain `spark-editor` next to `spark`; every archive has a `.sha256` that the installers verify.