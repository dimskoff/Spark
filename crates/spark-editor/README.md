# Spark Editor

Project manager, code editor, game runner and live debugger for Spark games (egui / eframe).
It is **not** a scene editor: a Spark game is plain code and stays that way.

```
spark-editor               welcome screen: recent projects, new game, open folder
spark-editor path/to/game  open a game (usually started as `spark editor .`)
```

Needs Rust 1.95+ (egui 0.36); the engine itself stays on 1.85. Build and start:

```
cargo build --release -p spark-cli -p spark-editor
target/release/spark editor path/to/game
```

`spark editor` looks for `spark-editor` next to the `spark` executable (override with `SPARK_EDITOR`).
The editor runs games with the `spark` that started it (`SPARK_EXE`).

## Layout of the code

| Module | Role |
|---|---|
| `app` | screens (welcome / workspace), settings, global window state |
| `welcome` | project manager: recent projects, new project, open, settings |
| `workspace/` | an open game: dockable panels - explorer, documents (code editor), console, problems, game / debugger, assets (previews), viewport |
| `process` | child processes: the game (`spark --devtools ADDR`), `spark check`, `spark build` |
| `logcap` | the logger; copies engine messages of the in-editor game to the console |
| `i18n` + `locales/*.toml` | every UI string (English and Russian) |
| `syntax`, `theme`, `toasts`, `settings`, `os`, `brand` | Luau highlighting, look and feel, notifications, persisted settings, OS helpers, logo |

## How it talks to the game

- **F5 (Run)** starts the game in its own process, exactly as players get it. The editor listens on a localhost TCP
  port and passes it with `--devtools 127.0.0.1:PORT`; the game connects back
  (protocol: crate `spark-devtools`, JSON lines, versioned).
- **Viewport** links the engine in-process (`engine = spark`): the game is drawn on egui's own wgpu device
  (`Renderer::from_device` + `render_to_view`), with a fly camera (RMB + WASD) and Play / Pause / Step / Stop.
- **Logs** are read from the game's stdout / stderr, not through the devtools link.

## Adding a UI string

Add the key to both `locales/en.toml` and `locales/ru.toml`; look it up through `i18n`. Missing keys fall back to English.
