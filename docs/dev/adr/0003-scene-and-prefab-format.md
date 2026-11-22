# ADR 0003: scene and prefab files are TOML (`.scene`, `.prefab`)

Status: proposed (Stage C). Roadmap: `analysis/11-roadmap.md` Stage C; design: `analysis/10-target-architecture.md` 10.2.

## Context

Spark has no scene format. The "scene tree" in the editor is a view of what scripts created (`Scene` in `spark-core/src/scene.rs`: slot arena of `Object`, `ObjectId = {index, generation}`), so nothing a human or an agent places can be saved, diffed or reviewed. Stage C needs a file that is:

- editable by a human in any text editor and by an AI agent with plain text edits (no tool required);
- diff-friendly: one property change = one changed line, stable ordering, no reflow of neighbours;
- able to express: stable object ids, parent/child hierarchy, per-object components, prefab instances with overrides, asset references by path (GUID later), a `format_version` with migrations, scene-level environment (sun, ambient, sky, fog, background; today these live on `Scene`, not on objects).

Facts about the tree that matter: `game.toml` already is TOML (parsed by hand in `spark-project`, `toml` 1.1 is a dependency of `spark-editor`); `Cargo.lock` already contains `toml`, `toml_edit`, `toml_parser`, `winnow`, `serde`, `serde_json`, `indexmap`; it does not contain `ron`. `spark-core` has no parser dependencies and must stay GPU-free (target 10.9).

## Decision

1. **TOML**, TOML 1.0 constructs only (no multi-line inline tables), UTF-8, LF. Extensions: `.scene` (a level) and `.prefab` (a reusable subtree). Same grammar, the header table differs (`[scene]` / `[prefab]`).
2. **Flat node list with `parent` references**, not nested tables. One `[[node]]` per object; components are sub-tables `[node.<component>]`; siblings keep file order. The writer emits depth-first pre-order (parent before children), so the file reads like the hierarchy.
3. **Stable ids are strings, unique per file**: `id = "a1f3c09e"` (8 lowercase hex by default, any `[a-z0-9_-]{1,32}` accepted so agents may write `id = "door_leaf"`). Not path-based: renaming or reparenting must not break references, overrides or diffs. Runtime `ObjectId` (slot + generation) is unrelated and never written. Duplicate or dangling ids are load errors with line numbers.
4. **Components** are the names of the type registry (ADR 0004): `transform`, `mesh`, `material`, `body`, `light`, `camera`, `script`, `tags`. A missing component = not present; a missing field = the registry default. The writer omits fields equal to their default, writes fields in registry declaration order, writes floats in the shortest round-trip form, normalizes `-0`. Rule: **changing a default is a breaking change** and needs a format migration (otherwise old scenes silently change).
5. **Value encodings** (the reflection `Value`, ADR 0004): vectors `[x, y, z]`; angles in **degrees** in files and the inspector (the Luau API keeps radians; the field's unit tells the layers apart); colors `"#rrggbb"` / `"#rrggbbaa"` when exactly representable, else `[r, g, b, a]` floats; enums as strings, data-bearing enums as inline tables with a `kind` key; node references as ids; assets as strings.
6. **Asset references** are project-relative, forward-slash paths with an optional `#fragment`: `"assets/hero.glb#Body"`. The field type is "asset path"; the reader also accepts `{ guid = "...", path = "..." }` so the GUID `.meta` work (analysis 10.4) is a reader/writer change, not a format break. Procedural meshes get a recipe (`{ primitive = "box", size = [1,1,1] }`); a mesh made at run time by `Mesh.custom` has no recipe and cannot be saved (the save reports it; it is never silently dropped).
7. **Prefabs are scenes with one root** (first `[[node]]`, no `parent`). An instance is a node with `prefab = "prefabs/door.prefab"`:
   - the instance node's own components override the prefab root's;
   - `[node.overrides]` overrides descendants, keys are `"<node id in the prefab>/<component>.<field>"`;
   - children of the instance that are added in the scene use `parent = "<instance id>"` (or `"<instance id>/<node id>"` to attach inside the prefab);
   - v1 supports property overrides and added children. Removing prefab nodes and nested-prefab overrides are out of scope for v1 (open question).
   - On save the engine computes overrides by diffing the live object against a fresh instantiation of the prefab, so there is no override bookkeeping to corrupt, and a property reset to the prefab value disappears from the file.
8. **Versioning**: `format_version = N` (integer) in the header. The loader parses to a raw TOML table, runs `migrate_N_to_N+1` steps (pure functions on the table, one per version, each with a golden fixture) until the current version, then decodes through the registry. Older files load with a warning; `spark migrate [--check]` rewrites them; a file newer than the engine fails with "update Spark" and the version it needs. Saving always writes the current version.
9. **Canonical writer, no comment preservation in v1**: `save` produces one deterministic byte sequence, so `load -> save` of a canonical file is byte-identical (the Stage C round-trip test). A leading `# ...` comment block is kept verbatim; other comments are not (open question, see Consequences).
10. **Crates**: parsing/printing and migrations live in a new `spark-scene` crate (depends on `toml`, `spark-reflect`, `spark-core`); `spark-core` owns the format-independent document model (`SceneDoc`, `Value`-based) and instantiate/capture. `spark-core` stays free of parsers.

### Example `levels/arena.scene`

```toml
# Arena: first test level
[scene]
format_version = 1
name = "Arena"

[environment]
ambient = "#59616f"
background = "#141a1f"

[environment.sun]
direction = [-0.4, -1.0, -0.3]
intensity = 1.0

[[node]]
id = "cam0"
name = "Camera"

[node.transform]
position = [0, 3, 8]
rotation = [-20, 0, 0]

[node.camera]
fov = 60
active = true

[[node]]
id = "lamp"
name = "Lamp"

[node.transform]
position = [2, 4, 0]

[node.light]
kind = { type = "spot", angle = 50, softness = 0.3 }
color = "#ffe9c4"
intensity = 3.0
range = 12
shadows = true

[[node]]
id = "crate"
name = "Crate"
tags = ["pickup", "wood"]

[node.transform]
position = [0, 0.5, 0]

[node.mesh]
primitive = "box"
size = [1, 1, 1]

[node.material]
color = "#d8a45a"
pbr = true
roughness = 0.6

[node.body]
kind = "dynamic"
mass = 4.0
bounciness = 0.2

[[node]]
id = "door1"
name = "Door A"
prefab = "prefabs/door.prefab"

[node.transform]
position = [4, 0, -2]

[node.overrides]
"leaf/material.color" = "#8a3b2f"
"leaf/transform.rotation" = [0, 90, 0]
```

### Example `prefabs/door.prefab`

```toml
[prefab]
format_version = 1
name = "Door"

[[node]]
id = "root"
name = "Door"

[node.script]
path = "src/door.luau"

[node.script.props]
open_angle = 90
speed = 2

[[node]]
id = "frame"
parent = "root"
name = "Frame"

[node.mesh]
asset = "assets/door.glb"
node = "Frame"

[node.body]
kind = "static"
shape = { kind = "auto" }

[[node]]
id = "leaf"
parent = "root"
name = "Leaf"

[node.transform]
position = [0.5, 1, 0]

[node.mesh]
asset = "assets/door.glb"
node = "Leaf"

[node.material]
color = "#a67c52"
```

(`tags` is a plain key of the node table because it is a list of strings; it is still the `tags` component in the registry.)

## Consequences

- Agents edit scenes with ordinary file edits, humans in any editor, reviewers read normal git diffs. Both `spark check` and the MCP `scene_read` can report errors with file, line, node id and did-you-mean (from the registry), the same quality as script errors (Stage A).
- TOML costs: no `null` (absence means default), arrays of tables are verbose for deep trees (mitigated by the flat list), numbers have no `f32` flavour (the writer is responsible for shortest round-trip text), and TOML 1.1 features must not be written because other tools target 1.0.
- A new runtime dependency on `toml` for games that call `scene.load` (small: `toml_parser`, `winnow`, `toml_datetime`; most of it is already compiled for the editor). To be measured when `spark-scene` lands (binary size, compile time).
- Comments inside a file are lost when the editor re-saves it. Acceptable for editor-owned scenes; if it hurts, switch the writer to `toml_edit` (already in `Cargo.lock`) to patch values in place. The format does not change.
- Merge conflicts: two people adding nodes append at different places and merge cleanly; two people editing the same line conflict as any text. Ids are random, so concurrent branches do not collide (a collision, ~1e-4 per 1000 nodes with 8 hex, is caught at load and at creation).
- "Defaults are omitted" makes files short but turns every default change into a migration; the test-suite needs a golden file per format version.

## Alternatives rejected

- **RON.** Maps to Rust types exactly and is in the Bevy ecosystem, but: not in the tree today; its Rust-flavoured syntax (`Some(..)`, tuple structs, `(` `)` for maps) is a second syntax to teach humans and LLMs, with no editor tooling outside Rust; pre-1.0 churn in the crate; a format we cannot read with a generic TOML/JSON tool. Its one real advantage (enums with data) is covered by inline tables with `kind`.
- **JSON.** Universal and already in the tree, but no comments, no trailing commas, every vector `[x, y, z]` is fine yet object nesting makes diffs noisy (closing braces and commas change when a neighbour is added), and it reads worse for humans. Kept as the wire format for devtools/MCP, not as the file format.
- **YAML.** Comments and brevity, but indentation sensitivity, implicit typing (`no` -> false, `1e3`), no dependency in the tree, and agents regularly produce subtly invalid indentation.
- **Nested tables for the hierarchy** (children inside parents). Reads like a tree, but a reparent rewrites the indentation/table paths of the whole subtree and makes diffs unreadable; TOML also makes deep `[[a.b.c]]` paths painful. Flat + `parent` is stable under reparenting of ancestors.
- **Path-based ids** (`/World/Door/Leaf`). Human-readable but renaming or moving a node breaks every reference and override; duplicate sibling names are legal in games.
- **A custom DSL or a binary format.** More work, no tooling, agents cannot edit it. A binary cooked format for shipping is a separate, later `spark build` concern.
- **Storing the runtime `ObjectId`** in files. It is slot + generation, meaningless across runs.
