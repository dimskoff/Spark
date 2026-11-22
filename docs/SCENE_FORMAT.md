# Scene and prefab files

A Spark level is a `.scene` file and a reusable group of objects is a `.prefab` file. Both are plain
[TOML](https://toml.io) (UTF-8, LF line endings), made to be edited by hand, by a script or by an AI agent with ordinary
text edits, and to produce small diffs: changing one property changes one line.

Spark writes every file in one canonical form (see "What the writer does"), so saving a file that is already
canonical gives back the same bytes. The design is in [`dev/adr/0003-scene-and-prefab-format.md`](dev/adr/0003-scene-and-prefab-format.md).

## A scene

```toml
# Arena: first test level
[scene]
format_version = 1
name = "Arena"

[environment]
ambient = "#59616f"
background = "#141a1f"

[environment.sun]
direction = [-0.5, -1, -0.25]
intensity = 1.2

[[node]]
id = "crate"
name = "Crate"

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
mass = 4.0
bounciness = 0.2
```

- `[scene]` (or `[prefab]` for a prefab) is the header. `format_version` is required; `name` is optional.
- `[environment]` is the scene's lighting and background: `ambient`, `background`, and the tables `[environment.sun]`
  (`direction`, `color`, `intensity`), `[environment.fog]` and `[environment.sky]`. A prefab has none.
- Every object is a `[[node]]`. The nodes form a **flat list**; the hierarchy is given by `parent`.

## Nodes

| Key | Meaning |
|---|---|
| `id` | Required. Unique in the file: 1 to 32 characters of `a-z`, `0-9`, `_`, `-`. Spark generates 8 hex digits (`"a1f3c09e"`); you may write your own (`"door_leaf"`). |
| `parent` | The `id` of the parent node. Missing = a root. A parent that does not exist, a duplicate id and a parent cycle are load errors with a line number. |
| `name` | Name shown in the editor and used by `find(name)`. |
| `visible` | `false` hides the node and its children. Default `true`. |
| `prefab` | A prefab instance: `"prefabs/door.prefab"`. |
| `tags` | A list of strings. |

Siblings keep their order in the file. Ids are never reused as references to something else, so renaming or moving a node
does not break anything that points at it.

## Components

A component is a sub-table of the node: `[node.transform]`, `[node.mesh]`, `[node.material]`, `[node.body]`,
`[node.light]` (and `[node.camera]`, `[node.script]`, which are read and kept but not used by the engine yet). A missing
component means the node does not have it; a missing field means the default. `transform` and `material` are always
there, so writing them is only needed to change something.

The fields are the ones of the Luau API, with the same ranges and validation. `spark api show` and the inspector list
them; a wrong name is reported with a suggestion:

```
line 8: unknown field `masss` on Body; did you mean `mass`? (node 'crate', [node.body])
```

### Value forms

| Kind | In the file |
|---|---|
| Vector | `[x, y, z]` |
| Angle | degrees (`rotation = [-20, 0, 0]`: pitch, yaw, roll). The Luau API uses radians; files and the inspector use degrees. |
| Color | `"#rrggbb"` or `"#rrggbbaa"`; `[r, g, b, a]` (0 to 1) for colors that are not exactly a byte value |
| Enum | `kind = "static"`, or with data `kind = { type = "spot", angle = 50.0, softness = 0.4 }` |
| Asset | a project-relative path with forward slashes, `"assets/hero.glb"`; `{ guid = "...", path = "..." }` is also read |
| Number | integers for integer fields; `1.0` style for float fields (an integer is accepted there, too) |

### Meshes

A mesh is a recipe, not vertices:

```toml
[node.mesh]
primitive = "sphere"          # cube, box, plane, quad, sphere, cylinder, triangles
radius = 0.5
segments = 24
lod = [{ distance = 30.0, primitive = "sphere", radius = 0.5, segments = 8 }]
```

or a mesh of a loaded model: `asset = "assets/door.glb"`, `node = "Frame"` and, for a node with several materials,
`part = 1` (the primitive index, default 0). A mesh recipe always lists its parameters. A mesh made by `Mesh.custom`
has no recipe and cannot be saved (see "Saving").

## Prefabs

A `.prefab` has a `[prefab]` header and one root node (the first `[[node]]`, without `parent`). An instance in a scene is a
node with `prefab = "prefabs/door.prefab"`; its own components override the prefab root's, and `[node.overrides]`
overrides descendants with keys like `"leaf/material.color"`. Prefab instances and `overrides` are read and written
unchanged by the current version, and are not instantiated by the engine yet.

## What the writer does

- One blank line before every table, one final newline, TOML 1.0 constructs only.
- Node keys in the order `id`, `parent`, `name`, `visible` (only when `false`), `prefab`, `tags`; then the component
  tables in the order `transform`, `mesh`, `material`, `body`, `light`, `camera`, `script`, `overrides`.
- Fields in the order the API documents them; fields equal to their default are left out; a present optional component with
  only defaults is written as an empty table (`[node.body]`); runtime-only fields (velocity, texture handles) are never written.
- Numbers use the shortest text that reads back as the same value. A float scalar always has a decimal point (`12.0`),
  vector components do not (`[0, 3, 8]`). `-0` is written `0`. Rotations are rounded to 4 decimals of a degree.
- A leading `# ...` comment block at the top of the file is kept. Other comments are lost when Spark rewrites a file.

## Saving

Some things have no file form yet: a mesh made by code (`Mesh.custom`), textures, shaders and cube maps (handles to things
loaded at run time). Saving reports them by node and component instead of dropping them silently:

```
2 thing(s) cannot be saved to a scene file:
  node 'a1f3c09e' ("Custom"), mesh: the mesh was made by code ... and has no recipe ...
```

Use a primitive or a model for the mesh to make the node saveable.

## Versions and `spark migrate`

`format_version` is the version of this format (currently **1**). A file with an older version still loads: Spark upgrades
it in memory and says so. A file with a newer version than your Spark is refused with the version it needs ("update Spark").

```
spark migrate levels/                 # rewrite every .scene / .prefab under levels/ to the current version
spark migrate --check .               # change nothing; exit code 1 if a file needs migration (for CI)
spark migrate --json levels/arena.scene
```

Files that are already current are never touched. Migrated files are written in the canonical form, so comments other
than the leading block are not kept.

Version history:

| Version | Change |
|---|---|
| 1 | First released format. (Version 0 was the draft: data-carrying enum tables named their variant with `kind` instead of `type`.) |

## For tools

The document model (`SceneFile`, `FileNode`, `capture`, `instantiate`) is in `spark-core` (`spark_core::doc`) and has no
parser; the `spark-scene` crate reads and writes the text (`spark_scene::parse`, `spark_scene::to_string`).
