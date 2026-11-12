//! New-game template (`spark new`, Spark Editor "New project") and the engine docs shipped into
//! every game (`spark docs`).
//!
//! ```text
//! mygame/
//!   main.luau        entry point: wires the modules together, stays short
//!   game.toml        title, window, build name
//!   src/             game code, one module per system (require("./src/player"))
//!   assets/          textures/ sounds/ models/ shaders/ fonts/
//!   AGENTS.md        instructions for AI coding agents
//!   docs/            the full Spark API reference + spark.d.luau type definitions, matching this engine version
//!   .luaurc, .vscode/settings.json   luau-lsp setup: autocomplete and type checking of the Spark API
//! ```

use std::path::Path;

const MAIN: &str = r##"-- {TITLE}: entry point. Keep this file short - it only wires the modules in src/ together.
-- Run:   spark .          (saving any file reloads the game instantly)
-- Ship:  spark build .    (one .exe with everything inside)

local Level = require("./src/level")
local Player = require("./src/player")
local Hud = require("./src/hud")

local player

function start()
    Level.build()
    player = Player.new(vec3(0, 0.5, 0))
end

function update(dt)
    player:update(dt)
end

function render()
    Hud.draw(player)
end
"##;

const LEVEL: &str = r##"-- The world: light, ground and a few props.

local Level = {}

local PROPS = 8

function Level.build()
    background("#9cc8f0")
    sun(vec3(-0.5, -1, -0.3))
    ambient("#4a5068")
    spawn(Mesh.plane(30), "#5a8f4e")
    for i = 1, PROPS do
        local a = i / PROPS * math.pi * 2
        spawn(Mesh.cube(), "#c8873a", { position = vec3(math.cos(a) * 6, 0.5, math.sin(a) * 6) })
    end
end

return Level
"##;

const PLAYER: &str = r##"-- The player: WASD to move, the camera follows.

local Player = {}
Player.__index = Player

local SPEED = 5
local CAMERA_OFFSET = vec3(0, 6, 9)

export type Player = typeof(setmetatable({} :: { body: any, walked: number }, Player))

function Player.new(position: vector): Player
    local self = setmetatable({}, Player)
    self.body = spawn(Mesh.cube(), "#e04848", { position = position, scale = vec3(0.6, 1, 0.6) })
    self.walked = 0
    return self
end

function Player.update(self: Player, dt: number)
    local move = vec3(0, 0, 0)
    if input.down("w") then move += vec3(0, 0, -1) end
    if input.down("s") then move += vec3(0, 0, 1) end
    if input.down("a") then move += vec3(-1, 0, 0) end
    if input.down("d") then move += vec3(1, 0, 0) end
    -- gamepad left stick (0 inside the dead zone, up = +1)
    move += vec3(input.pad_axis("left_x"), 0, -input.pad_axis("left_y"))
    if length(move) > 0 then
        local step = (if length(move) > 1 then normalize(move) else move) * SPEED * dt
        self.body.position += step
        self.walked += length(step)
    end
    camera.position = self.body.position + CAMERA_OFFSET
    camera:look_at(self.body)
end

return Player
"##;

const HUD: &str = r##"-- 2D overlay, drawn every frame from render().

local Hud = {}

function Hud.draw(player)
    draw.text(string.format("walked: %.1f m", player.walked), 20, 20, { size = 28, shadow = true })
    draw.text("WASD / left stick - move", 20, 56, { size = 18, color = "#ffffffaa" })
end

return Hud
"##;

const TOML: &str = r##"# Spark game manifest (all keys optional, see docs/SPARK_API.md).
title = "{TITLE}"
spark = "{SPARK}"         # engine version the game is made for
width = 1280
height = 720
# fullscreen = false
# icon = "assets/icon.png"
# splash = false          # "Powered by Spark" intro
# build_name = "MyGame"   # exe name of `spark build`
exclude = ["AGENTS.md", "docs"]  # not packed into the build
"##;

/// The starter scenario test of the 2D template: a 2D game is tested through what it draws.
const TEST_2D: &str = r##"-- Scenario test: plays the game headless and checks what it drew. Run: spark test .

test.step("the HUD shows the score")
test.wait_text("Coins: 0")
test.step("the player moves right")
test.hold("d", 0.5)
test.expect_text("Coins:")
"##;

/// The starter scenario test of the 3D template (`spark test .`): shows an agent what a test looks like.
const TEST_3D: &str = r##"-- Scenario test: plays the game headless and checks the result. Run: spark test .
-- Add one test per fixed bug / finished feature; see "Scenario tests" in docs/SPARK_API.md.

test.step("the player walks right and the camera follows")
test.wait(0.2)
local start_x = camera.position.x
test.hold("d", 1)
test.expect(camera.position.x > start_x + 3, "the camera should follow the player to the right")
"##;

const GITIGNORE: &str ="dist/\nscreenshots/\nerror.log\n";

const AGENTS: &str = r##"# AGENTS.md - working on this game

This is a game for **Spark Engine**: a native runtime (Rust + wgpu) where all game code is **Luau**.
There is no scene editor - the scene, logic, UI and assets are all described in code
(the optional Spark Editor is only a project manager, code editor, runner and live debugger).

## Read first

- `docs/SPARK_API.md` - the complete Luau API of the engine version this game was made with.
  Use only what is listed there: unknown functions, options and keys raise errors that name the allowed ones.
- `docs/SPARK_SHADERS.md` - custom WGSL surface and post-process shaders.
- `docs/spark.d.luau` - type definitions of the same API (luau-lsp). Editors with luau-lsp show wrong
  names / arguments as you type (`.vscode/settings.json` and `.luaurc` are set up for it).

## Layout

- `main.luau` - entry point. Only wires modules together in `start / update / fixed_update / late_update / render`.
- `src/` - one module per system: `local Player = require("./src/player")`, each file returns a table.
  Shared state lives in its own module (e.g. `src/state.luau`). Keep files under ~800 lines.
- `assets/` - textures, sounds, models (glTF), shaders, fonts. Paths are relative to the game folder.
- `game.toml` - title, window size, icon, build name, `spark` = the engine version the game is made for.
  After `spark update` to a new minor version, run `spark docs .`, adapt the code to the new API, then raise `spark`.

## Check your work (no window needed)

```
spark check .                    # compiles every .luau file and runs 60 frames headless (no GPU): file:line: message
spark check . --frames 300 --json   # {"ok":false,"errors":[{"file":"src/player.luau","line":12,"message":"..."}],...}
spark check . --types           # also type-checks every file with luau-lsp (wrong API names / arguments), if installed
spark . --headless --frames 120 --screenshot shot.png   # render a frame, then look at shot.png
spark . --headless --frames 60 --dump-scene            # every object: position, rotation, bounds
```

`spark check` and a headless run exit with code 1 on the first script error and print `file:line`, the message and a hint.
Fix every error before moving on. `print(...)` goes to the log.

## Scenario tests (spark test) - the main way to verify gameplay

```
spark test .                 # every tests/*.luau: PASS / FAIL file:line: message + at step + snapshot diff
spark test . door            # only tests whose path contains "door"
spark test . --update        # accept snapshot changes you meant
```

A test plays the game headless (fast, no GPU) with `test.tap("e")`, `test.wait(2)`, `test.find("door")`,
`test.eq(a, b, msg)`, `test.snapshot("name", state)`; see "Scenario tests" in `docs/SPARK_API.md`.
Any script error of the game fails the test. Snapshots catch side effects: everything named in the world is compared.

Rules:
- Do NOT play the game in a window or take screenshots to find logic bugs. Reproduce the bug in a test first
  (it must FAIL), then fix it, then `spark check . && spark test .` must pass. Screenshots only for visual problems.
- Every fixed bug and every finished chapter / quest step gets a test. Never delete or weaken a test to make it pass.
- A snapshot diff you did not intend is a bug, not something to `--update`.
- Start tests at the place you need through game helpers (e.g. `goto_chapter(3)`), not by replaying everything.

## Run and ship

```
spark .            # window, the game reloads on every save
spark build .      # dist/<Name>.exe - one file with everything packed inside
```

## Conventions

Y-up, right-handed, 1 unit = 1 meter, objects look along -Z, angles in radians (camera FOV in degrees),
colors as `"#rrggbb"` strings or `Color`.
"##;

/// Engine docs shipped into every game, so the AI agent always has the API of this exact version.
const DOCS: &[(&str, &str)] = &[
    ("docs/SPARK_API.md", include_str!("../../../docs/SPARK_API.md")),
    ("docs/SPARK_SHADERS.md", include_str!("../../../docs/SPARK_SHADERS.md")),
    ("docs/spark.d.luau", include_str!("../../../docs/spark.d.luau")),
];

/// Editor setup for luau-lsp, written only when missing (the user may change them).
const EDITOR: &[(&str, &str)] = &[(".luaurc", LUAURC), (".vscode/settings.json", VSCODE)];

/// `require("@game/src/x")` resolves from the game folder, like the engine does. Engine callbacks
/// (`function start()`, `update`, ...) are called by name, so "unused function" warnings are off.
const LUAURC: &str = r#"{
    "languageMode": "nonstrict",
    "lint": {
        "FunctionUnused": false
    },
    "aliases": {
        "game": "."
    }
}
"#;

/// VS Code + "Luau Language Server" (JohnnyMorganz.luau-lsp): the Spark API from docs/spark.d.luau.
const VSCODE: &str = r#"{
    "luau-lsp.platform.type": "standard",
    "luau-lsp.types.definitionFiles": {
        "@spark": "docs/spark.d.luau"
    },
    "luau-lsp.sourcemap.enabled": false,
    "luau-lsp.require.mode": "relativeToFile",
    "files.associations": {
        "*.luau": "luau"
    }
}
"#;

const ASSET_DIRS: &[&str] = &["textures", "sounds", "models", "shaders", "fonts"];

fn write_file(p: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("cannot create '{}': {e}", parent.display()))?;
    }
    std::fs::write(p, text).map_err(|e| format!("cannot write '{}': {e}", p.display()))
}

/// Engine docs are always overwritten (they belong to the engine); AGENTS.md and the editor setup only if
/// missing (the user may edit them).
pub fn write_docs(dir: &Path) -> Result<(), String> {
    for (rel, text) in DOCS {
        write_file(&dir.join(rel), text)?;
    }
    for (rel, text) in [("AGENTS.md", AGENTS)].iter().chain(EDITOR) {
        let p = dir.join(rel);
        if !p.exists() {
            write_file(&p, text)?;
        }
    }
    Ok(())
}

/// "0.1" for engine 0.1.3: the API version a new game targets.
pub fn engine_minor() -> String {
    spark_core::VERSION.split('.').take(2).collect::<Vec<_>>().join(".")
}

/// Which starter game `spark new` writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Template {
    /// A small 3D scene with a player, a level and a HUD, one module per system (the default).
    #[default]
    D3,
    /// A 2D game drawn only with `draw.*`.
    D2,
    /// Only the skeleton: `main.luau` with the callbacks, `game.toml`, the folders and the docs.
    Empty,
}

impl Template {
    /// `3d`, `2d` or `empty` (the values of `spark new --template`).
    pub fn parse(name: &str) -> Result<Self, String> {
        match name {
            "3d" => Ok(Self::D3),
            "2d" => Ok(Self::D2),
            "empty" => Ok(Self::Empty),
            other => Err(format!("unknown template '{other}' (use 3d, 2d or empty)")),
        }
    }
}

const MAIN_2D: &str = r##"-- {TITLE}: a 2D game drawn with draw.* only. Coordinates are virtual pixels: origin top-left, Y down.
-- Run:   spark .          (saving any file reloads the game instantly)
-- Ship:  spark build .    (one .exe with everything inside)

local SPEED = 420
local player = { x = 640, y = 560, size = 40 }
local score = 0
local coin = { x = 300, y = 300, r = 14 }

local function move_coin()
    coin.x = 60 + math.random() * (screen.width - 120)
    coin.y = 60 + math.random() * (screen.height - 200)
end

function update(dt)
    local dx, dy = 0, 0
    if input.down("a") or input.down("left") then dx -= 1 end
    if input.down("d") or input.down("right") then dx += 1 end
    if input.down("w") or input.down("up") then dy -= 1 end
    if input.down("s") or input.down("down") then dy += 1 end
    dx += input.pad_axis("left_x")
    dy -= input.pad_axis("left_y")
    player.x = math.clamp(player.x + dx * SPEED * dt, 0, screen.width)
    player.y = math.clamp(player.y + dy * SPEED * dt, 0, screen.height)
    local ox, oy = player.x - coin.x, player.y - coin.y
    if ox * ox + oy * oy < (player.size * 0.5 + coin.r) ^ 2 then
        score += 1
        move_coin()
    end
end

function render()
    draw.rect(player.x, player.y, player.size, player.size, "#5ad1ff", { align = "center" })
    draw.circle(coin.x, coin.y, coin.r, "#ffd34d")
    draw.text("Coins: " .. score, 20, 20, { size = 32, shadow = true })
    draw.text("WASD / arrows / left stick - move", 20, 62, { size = 18, color = "#ffffffaa" })
end
"##;

const MAIN_EMPTY: &str = r##"-- {TITLE}: entry point. Keep this file short - move systems into src/ modules (require("./src/name")).
-- Run:   spark .          (saving any file reloads the game instantly)
-- Ship:  spark build .    (one .exe with everything inside)

function start()
    -- build the world here: spawn(...), camera, lights
end

function update(dt)
    -- game logic, once per frame
end

function render()
    -- 2D overlay: draw.text(...), draw.rect(...)
end
"##;

/// Writes the default (3D) starter game; see [`write_template_kind`].
pub fn write_template(dir: &Path, title: &str) -> Result<(), String> {
    write_template_kind(dir, title, Template::D3)
}

pub fn write_template_kind(dir: &Path, title: &str, kind: Template) -> Result<(), String> {
    let t = title.replace('"', "'");
    let toml = TOML.replace("{TITLE}", &t).replace("{SPARK}", &engine_minor());
    let files: Vec<(&str, String)> = match kind {
        Template::D3 => vec![
            ("main.luau", MAIN.replace("{TITLE}", &t)),
            ("game.toml", toml),
            ("src/level.luau", LEVEL.into()),
            ("src/player.luau", PLAYER.into()),
            ("src/hud.luau", HUD.into()),
            ("tests/smoke.luau", TEST_3D.into()),
            (".gitignore", GITIGNORE.into()),
        ],
        Template::D2 => vec![
            ("main.luau", MAIN_2D.replace("{TITLE}", &t)),
            ("game.toml", toml),
            ("tests/smoke.luau", TEST_2D.into()),
            (".gitignore", GITIGNORE.into()),
        ],
        Template::Empty => vec![
            ("main.luau", MAIN_EMPTY.replace("{TITLE}", &t)),
            ("game.toml", toml),
            (".gitignore", GITIGNORE.into()),
        ],
    };
    for (rel, text) in files {
        let p = dir.join(rel);
        if !p.exists() {
            write_file(&p, &text)?;
        }
    }
    write_docs(dir)?;
    for d in ASSET_DIRS {
        let p = dir.join("assets").join(d);
        std::fs::create_dir_all(&p).map_err(|e| format!("cannot create '{}': {e}", p.display()))?;
        let keep = p.join(".gitkeep");
        if !keep.exists() {
            let _ = std::fs::write(keep, "");
        }
    }
    Ok(())
}
