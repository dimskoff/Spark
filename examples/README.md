# Examples

Every example is a game folder: run it with `spark examples/<name>`, check it with `spark check examples/<name>`.
CI runs `spark check` on all of them, runs the scenario test, and compares a headless render with `tests/golden.png`.

| Folder | One feature |
|---|---|
| `01-hello` | the smallest 3D game (ground, cube, sun, sky) |
| `02-input` | keyboard, mouse, gamepad |
| `03-physics` | rigid bodies, collisions, throwing |
| `04-draw-2d` | a 2D game with `draw.*` only |
| `05-ui` | slider, checkbox, buttons, panel |
| `06-sound` | one-shots, loops, 3D sound |
| `07-save` | data that survives restarts |
| `08-timers-tweens` | `every`, tasks with `wait`, tweens |
| `09-shaders` | custom WGSL surface and post shaders |
| `10-scenario-test` | `spark test`: a test that presses a key and checks the result |
| `showcase/pbr-gallery` | PBR material grid, sky lighting, ACES, bloom, SSAO |
| `showcase/night-lights` | many colored lights, shadows, fog, bloom |
| `demo` | a bigger 3D playground: physics, shaders, sound, fly camera |
| `hello` | the same idea in pure Rust (`cargo run -p hello`) |

Refresh a golden image after an intended visual change: `spark golden examples/<name> --update --frames 40 --size 480x270`, then look at it.
