# Spark Luau API reference

Generated from `docs/spark.d.luau` for Spark 0.1.0. Do not edit by hand: edit the definition file and run
`SPARK_BLESS=1 cargo test -p spark-api`. Narrative guides, units and conventions are in `SPARK_API.md`.

## Global functions

### `vec3`

```luau
vec3(x: number?, y: number?, z: number?) -> vector
```

### `length`

```luau
length(v: vector) -> number
```

### `normalize`

```luau
normalize(v: vector) -> vector
```

### `dot`

```luau
dot(a: vector, b: vector) -> number
```

### `cross`

```luau
cross(a: vector, b: vector) -> vector
```

### `distance`

```luau
distance(a: vector, b: vector) -> number
```

### `lerp`

```luau
lerp<T>(a: T, b: T, t: number) -> T
```

### `spawn`

```luau
spawn(what: Mesh | Model | string, material: (Material | Texture | ColorLike | SpawnProps)?, props: SpawnProps?) -> Object
```

### `group`

```luau
group(props: SpawnProps?) -> Object
```

### `light`

```luau
light(spec: LightSpec) -> Object
```

### `find`

```luau
find(name: string) -> Object?
```

### `find_all`

```luau
find_all(name: string) -> { Object }
```

### `destroy`

```luau
destroy(obj: Object)
```

### `dump_scene`

```luau
dump_scene() -> string
```

### `sun`

```luau
sun(direction: vector, color: ColorLike?, intensity: number?)
```

### `ambient`

```luau
ambient(color: ColorLike)
```

### `background`

```luau
background(color: ColorLike)
```

### `fog`

```luau
fog(color: ColorLike?, near: number?, far: number?)
```

### `sky`

```luau
sky(spec: (SkySpec | boolean)?)
```

### `after`

```luau
after(seconds: number, fn: () -> (), owner: Object?) -> Timer
```

### `every`

```luau
every(seconds: number, fn: (n: number) -> (), opts: (number | { times: number?, owner: Object? })?) -> Timer
```

### `next_frame`

```luau
next_frame(fn: () -> (), owner: Object?) -> Timer
```

### `after_frames`

```luau
after_frames(n: number, fn: () -> (), owner: Object?) -> Timer
```

### `task`

```luau
task(fn: (...any) -> (), ...: any) -> Task
```

### `wait`

```luau
wait(seconds: number?) -> number
```

### `wait_frames`

```luau
wait_frames(n: number?)
```

### `wait_until`

```luau
wait_until(condition: () -> boolean, timeout: number?) -> boolean?
```

### `wait_event`

```luau
wait_event(name: string, timeout: number?) -> ...any
```

### `on`

```luau
on(name: string, fn: (...any) -> any, opts: ListenerOptions?) -> Listener
```

### `once`

```luau
once(name: string, fn: (...any) -> any, opts: ListenerOptions?) -> Listener
```

### `emit`

```luau
emit(name: string, ...: any) -> boolean
```

### `emit_later`

```luau
emit_later(name: string, ...: any)
```

### `off`

```luau
off(listener: Listener | string)
```

### `tween`

```luau
tween(target: any, goals: { [string]: any }, seconds: number, ease: (Ease | TweenOptions)?) -> Tween
```

### `raycast`

```luau
raycast(origin: vector, dir: vector, max: number?, opts: { ignore: Object? }?) -> Hit?
```

### `pick`

```luau
pick(origin: vector, dir: vector, max: number?, opts: { ignore: Object? }?) -> Hit?
```

### `quit`

```luau
quit()
```

### `restart`

```luau
restart()
```

## Global values

- `camera: Camera`

## Tables

### `Mesh`

```luau
Mesh.cube(size: number?) -> Mesh
Mesh.box(w: number?, h: number?, d: number?, opts: MeshOptions?) -> Mesh
Mesh.plane(w: number?, d: number?, opts: MeshOptions?) -> Mesh
Mesh.quad(w: number?, h: number?, opts: MeshOptions?) -> Mesh
Mesh.sphere(radius: number?, segments: number?) -> Mesh
Mesh.cylinder(radius: number?, height: number?, segments: number?) -> Mesh
Mesh.triangles(points: { vector }) -> Mesh
Mesh.custom(positions: { vector }, indices: { number }, uvs: { number }?, normals: { vector }?) -> Mesh
```

### `Texture`

```luau
Texture.load(path: string) -> Texture
Texture.checker(a: ColorLike?, b: ColorLike?, cells: number?, size: number?) -> Texture
Texture.solid(color: ColorLike) -> Texture
```

### `Material`

```luau
Material.color(color: ColorLike) -> Material
Material.unlit(color: ColorLike) -> Material
Material.texture(texture: Texture | string, tint: ColorLike?) -> Material
Material.checker(a: ColorLike, b: ColorLike, repeats: number?) -> Material
Material.pbr(color: ColorLike?, metallic: number?, roughness: number?) -> Material
```

### `Color`

```luau
Color.rgb(r: number, g: number, b: number) -> Color
Color.rgba(r: number, g: number, b: number, a: number) -> Color
Color.hex(hex: string) -> Color
Color.lerp(a: ColorLike, b: ColorLike, t: number) -> Color
Color.WHITE: Color
Color.BLACK: Color
Color.GRAY: Color
Color.GREY: Color
Color.RED: Color
Color.GREEN: Color
Color.BLUE: Color
Color.YELLOW: Color
Color.ORANGE: Color
Color.PURPLE: Color
Color.PINK: Color
Color.BROWN: Color
Color.SKY: Color
```

### `Model`

```luau
Model.load(path: string) -> Model
```

### `Shader`

```luau
Shader.load(path: string) -> Shader
Shader.new(code: string, name: string?) -> Shader
```

### `Font`

```luau
Font.load(path: string) -> Font
```

### `graphics`

```luau
graphics.height: number?
graphics.scale: number
graphics.upscale: "linear" | "nearest"
graphics.filter: "linear" | "nearest"
graphics.msaa: number
graphics.shadows: boolean
graphics.shadow_distance: number
graphics.shadow_size: number
graphics.light_shadows: number
graphics.light_shadow_size: number
graphics.light_shadow_updates: number
graphics.light_shadow_soft: boolean
graphics.scene_width: number
graphics.scene_height: number
graphics.shader: Shader?
graphics.post: any
graphics.tonemap: "off" | "aces" | "reinhard" | "hejl"
graphics.fxaa: boolean
graphics.bloom: BloomSettings?
graphics.ssao: SsaoSettings?
graphics.motion_blur: MotionBlurSettings?
graphics.dof: DofSettings?
graphics.color_grading: ColorGradingSettings?
graphics.chromatic_aberration: ChromaticAberrationSettings?
graphics.film_grain: FilmGrainSettings?
graphics.vignette: VignetteSettings?
```

- `shader`: Shader, list of Shaders / { shader = s, size = "screen" | "scene" }, or nil
- `post`: built-in post effects: nil / false = off, true = defaults, table = settings

### `physics`

```luau
physics.gravity: vector
physics.enabled: boolean
```

### `sound`

```luau
sound.play(path: string, opts: SoundOptions?) -> Sound
sound.bus(name: string) -> Bus
sound.volume: number
sound.stop_all(fade: number?)
sound.enabled() -> boolean
```

### `draw`

```luau
draw.rect(x: number, y: number, w: number, h: number, color: ColorLike?, opts: { align: Align?, line: number? }?)
draw.circle(x: number, y: number, r: number, color: ColorLike?, opts: { line: number? }?)
draw.line(x1: number, y1: number, x2: number, y2: number, color: ColorLike?, width: number?)
draw.polygon(points: { number }, color: ColorLike?, opts: { line: number? }?)
draw.sprite(texture: Texture | string, x: number, y: number, opts: { w: number?, h: number?, scale: number?, rotation: number?, color: ColorLike?, align: Align?, flip_x: boolean?, flip_y: boolean?, region: { number }?, grid: { number }?, frame: number?, filter: ("nearest" | "linear")?, }?)
draw.text(text: string | number, x: number, y: number, opts: TextOptions?) -> (number, number)
draw.measure(text: string | number, opts: { size: number?, font: Font?, width: number? }?) -> (number, number)
draw.push()
draw.pop()
draw.translate(x: number, y: number)
draw.rotate(radians: number)
draw.scale(sx: number, sy: number?)
draw.layer(name: ("ui" | "scene")?) -> string
draw.filter(mode: "nearest" | "linear")
draw.stats() -> { [string]: number }
draw.spark_badge(opts: { align: Align?, margin: number?, width: number?, alpha: number?, x: number?, y: number? }?) -> (boolean, boolean)
```

### `ui`

```luau
ui.button(id: WidgetId, x: number, y: number, w: number, h: number, text: string) -> boolean
ui.slider(id: WidgetId, x: number, y: number, w: number, h: number, value: number, min: number?, max: number?) -> number
ui.checkbox(id: WidgetId, x: number, y: number, size: number, checked: boolean) -> boolean
ui.panel(x: number, y: number, w: number, h: number, color: ColorLike?, border: ColorLike?) -> boolean
ui.label(x: number, y: number, text: string, size: number?, color: ColorLike?)
ui.image(x: number, y: number, w: number, h: number, texture: Texture | string)
ui.progress(x: number, y: number, w: number, h: number, t: number, color: ColorLike?)
ui.hovered() -> boolean
```

### `screen`

```luau
screen.width: number
screen.height: number
screen.scale: number
screen.pixel_width: number
screen.pixel_height: number
screen.mouse() -> (number, number)
screen.center() -> (number, number)
```

### `input`

```luau
input.down(key: Key) -> boolean
input.pressed(key: Key) -> boolean
input.released(key: Key) -> boolean
input.repeated(key: Key) -> boolean
input.text() -> string
input.mouse_down(button: MouseButton?) -> boolean
input.mouse_pressed(button: MouseButton?) -> boolean
input.mouse_released(button: MouseButton?) -> boolean
input.mouse() -> (number, number)
input.mouse_delta() -> (number, number)
input.wheel() -> number
input.lock_mouse(on: boolean?)
input.mouse_locked() -> boolean
input.pad_down(button: PadButton, pad: number?) -> boolean
input.pad_pressed(button: PadButton, pad: number?) -> boolean
input.pad_released(button: PadButton, pad: number?) -> boolean
input.pad_axis(axis: PadAxis, pad: number?) -> number
input.pad_stick(stick: "left" | "right", pad: number?) -> (number, number)
input.pads() -> number
input.pad_name(pad: number?) -> string?
input.any_pressed() -> boolean
```

- `released`: went down or auto-repeated this frame (key held): menus, text fields
- `repeated`: text typed this frame (keyboard layout and IME aware, no control characters); "" if none

### `time`

```luau
time.dt: number
time.elapsed: number
time.frame: number
time.fps: number
time.unscaled_dt: number
time.unscaled_elapsed: number
time.scale: number
time.fixed_dt: number
```

### `test`

Exists only in files under tests/ run by `spark test`.

```luau
test.press(key: Key)
test.release(key: Key)
test.tap(key: Key)
test.hold(key: Key, seconds: number?)
test.mouse(x: number, y: number)
test.button(name: string, down: boolean)
test.click(x: number?, y: number?, button: string?)
test.text(s: string)
test.type(s: string)
test.wait(seconds: number?) -> number
test.frames(n: number?)
test.wait_until(condition: () -> boolean, timeout: number?, what: string?)
test.wait_event(name: string, timeout: number?) -> ...any
test.find(name: string, timeout: number?) -> Object
test.expect<T>(cond: T, msg: string?) -> T
test.eq(actual: any, expected: any, msg: string?)
test.near(actual: any, expected: any, eps: number?, msg: string?)
test.fail(msg: string?)
test.step(name: string)
test.snapshot(name: string, state: any?)
test.texts() -> { string }
test.sees(s: string) -> boolean
test.expect_text(s: string, msg: string?)
test.wait_text(s: string, timeout: number?)
```

- `snapshot`: every text drawn this frame (draw.text and ui), in draw order; shadow copies are listed once
- `texts`: true when s is part of any drawn text (reads the last finished frame: call test.frames(1) after a state change)

### `save`

```luau
save.set(key: string, value: any)
save.get<T>(key: string, default: T?) -> T
save.has(key: string) -> boolean
save.delete(key: string)
save.clear()
save.keys() -> { string }
save.flush()
save.path() -> string?
```

### `discord`

```luau
discord.connect(app_id: string)
discord.set(presence: { details: string?, state: string?, large_image: string?, large_text: string?, small_image: string?, small_text: string?, start: (boolean | number)?, finish: number?, buttons: { { label: string, url: string } }?, })
discord.clear()
discord.shutdown()
discord.status() -> "off" | "connecting" | "connected"
```

### `window`

```luau
window.fullscreen: boolean
window.title: string?
window.width: number
window.height: number
```

### `debug`

Spark's debug overlay merged with the standard Luau debug library.

```luau
debug.enabled: boolean
debug.hotkey: boolean
debug.show_fps: boolean
debug.show_gpu: boolean
debug.show_scene: boolean
debug.show_physics: boolean
debug.fps: number
debug.frame_time: number
debug.toggle() -> boolean
debug.avg_frame_ms() -> number
debug.add_line(text: string, color: ColorLike?)
debug.clear_lines()
debug.traceback(message: string?, level: number?) -> string
debug.info(...any) -> ...any
```

- `fps`: read-only
- `frame_time`: read-only (ms)

## Classes

### `Color`

```luau
color.r: number
color.g: number
color.b: number
color.a: number
color:lerp(other: ColorLike, t: number) -> Color
color:with_alpha(a: number) -> Color
```

### `Mesh`

```luau
```

### `Texture`

```luau
```

### `Material`

```luau
material.blend: BlendMode
material.double_sided: boolean
material.cast_shadow: boolean
material.pbr: boolean
material.metallic: number
material.roughness: number
material.emissive: Color
material.normal_map: Texture?
material:tiling(x: number, y: number?) -> Material
material:with_color(color: ColorLike) -> Material
material:with_unlit(on: boolean?) -> Material
material:with_blend(mode: BlendMode) -> Material
material:with_double_sided(on: boolean?) -> Material
material:with_cast_shadow(on: boolean?) -> Material
material:with_metallic(metallic: number) -> Material
material:with_roughness(roughness: number) -> Material
material:with_emissive(color: ColorLike) -> Material
material:with_normal_map(texture: (Texture | string)?) -> Material
material:with_shader(shader: Shader?) -> Material
material:with_data(a: number, b: number?, c: number?, d: number?) -> Material
```

### `Model`

```luau
model.nodes: { string }
model.name: string
model.animations: { string }
model.size: vector
model:duration(clip: string) -> number
```

### `Shader`

```luau
shader.name: string
shader.kind: "surface" | "post"
shader.params: { string }
shader.vertex: boolean
shader.texture1: (Texture | string)?
shader.texture2: (Texture | string)?
shader.[string]: any
shader:set(values: { [string]: any })
```

- `vertex`: false = ignore the shader's `fn vertex` (engine vertex stage, plain depth: early depth testing stays on)
- `[string]`: `shader.<param>` reads / writes a field of `struct Params` in the WGSL code (any name).

### `Font`

```luau
```

### `Timer`

```luau
timer.active: boolean
timer.paused: boolean
timer.remaining: number
timer:cancel()
timer:pause()
timer:resume()
```

### `Task`

```luau
task.status: "running" | "waiting" | "done" | "cancelled" | "failed"
task.alive: boolean
task:cancel()
```

### `Listener`

```luau
listener.active: boolean
listener:off()
```

### `Tween`

```luau
tween.active: boolean
tween.finished: boolean
tween:cancel()
```

### `Sound`

```luau
sound.playing: boolean
sound:stop(fade: number?)
sound:set_volume(volume: number, fade: number?)
sound:set_pitch(pitch: number, fade: number?)
sound:set_pan(pan: number)
sound:set_position(at: vector | Object)
```

### `Bus`

```luau
bus.name: string
bus.volume: number
bus.paused: boolean
bus:set_volume(volume: number, fade: number?)
bus:pause(fade: number?)
bus:resume(fade: number?)
bus:stop(fade: number?)
```

### `LightRef`

```luau
lightRef.type: "point" | "spot"
lightRef.color: Color
lightRef.intensity: number
lightRef.range: number
lightRef.shadows: boolean
lightRef.angle: number
lightRef.softness: number
```

- `type`: Point or spot (Luau: `light.type`, `"point"` / `"spot"`).
- `color`: Light color.
- `intensity`: Brightness multiplier (0 = off). Negative values are clamped to 0. At least 0.
- `range`: Distance in meters where the light has faded out completely. Values below 0.01 are clamped. At least 0.01 m.
- `shadows`: Casts shadows (a cube shadow map, see `RenderSettings::light_shadows`).
- `angle`: Full cone angle of a spot light in degrees, 1 to 179 (new spot lights: 45). Used by "spot".
- `softness`: 0..1 = blurry edge. Range 0 to 1. Used by "spot".

### `Object`

```luau
object.id: number
object.x: number
object.y: number
object.z: number
object.material: Material?
object.parent: Object?
object.light: LightRef?
object.body: BodyType?
object.name: string
object.visible: boolean
object.position: vector
object.rotation: vector
object.scale: vector
object.mesh: Mesh?
object.lod: { LodLevel }
object.color: Color
object.shader: Shader?
object.data: { number }
object.blend: BlendMode
object.double_sided: boolean
object.cast_shadow: boolean
object.pbr: boolean
object.metallic: number
object.roughness: number
object.emissive: Color
object.normal_map: Texture?
object.orm_map: Texture?
object.mass: number
object.sensor: boolean
object.gravity_scale: number
object.velocity: vector
object.angular_velocity: vector
object:rotate_x(angle: number)
object:rotate_y(angle: number)
object:rotate_z(angle: number)
object:translate(delta: vector)
object:look_at(target: vector | Object)
object:place_on(other: Object)
object:set_parent(parent: Object?)
object:children() -> { Object }
object:world_position() -> vector
object:forward() -> vector
object:right() -> vector
object:up() -> vector
object:bounds() -> (vector?, vector?)
object:size() -> vector
object:distance_to(other: Object | vector) -> number
object:exists() -> boolean
object:destroy()
object:find(name: string) -> Object?
object:paint(material: Material | ColorLike)
object:on(name: string, fn: (...any) -> any, opts: ListenerOptions?) -> Listener
object:once(name: string, fn: (...any) -> any, opts: ListenerOptions?) -> Listener
object:emit(name: string, ...: any) -> boolean
object:emit_later(name: string, ...: any)
object:after(seconds: number, fn: () -> ()) -> Timer
object:every(seconds: number, fn: (n: number) -> (), times: number?) -> Timer
object:play(clip: string, opts: PlayOptions?)
object:stop(fade: number?)
object:is_playing(clip: string?) -> boolean
object:animation() -> string?
object:animation_time() -> number
object:set_animation_speed(speed: number)
object:animations() -> { string }
object:add_body(spec: (BodyType | boolean | BodySpec)?)
object:remove_body()
object:impulse(v: vector)
object:force(v: vector)
object:torque(v: vector)
object:move(delta: vector) -> (vector, boolean)
```

- `x`: shortcuts for position.x / .y / .z
- `body`: physics (see "Physics"): `nil` / false removes the body; the scalars (mass, velocity, ...) are generated below
- `name`: Name shown in the hierarchy and used by `find(name)`.
- `visible`: Drawn (children inherit it being off). Default true.
- `position`: Position in meters. Unit: m. Default vec3(0, 0, 0).
- `rotation`: Rotation as Euler angles in radians: (pitch, yaw, roll) = rotation around X, Y, Z, applied Y, then X, then Z. Relative to the parent.
- `scale`: Scale per axis (1 = original size). Default vec3(1, 1, 1).
- `mesh`: The mesh drawn (a runtime handle). Default "runtime:mesh/0".
- `lod`: Level of detail: from each distance on the renderer draws that mesh instead of `mesh`.
- `color`: Base color (multiplied with the texture). Default rgba(1, 1, 1, 1).
- `shader`: Custom surface shader (`nil` = the engine default).
- `data`: Free per-material numbers for custom shaders (`object.data` in WGSL). Default vec4(0, 0, 0, 0).
- `blend`: Blending with the background (see `BlendMode`). Default "auto".
- `double_sided`: Draw back faces too (leaves, flags, planes seen from below). Default false.
- `cast_shadow`: Casts a shadow from the sun (`RenderSettings::shadows`). Default true.
- `pbr`: Physically based shading (Cook-Torrance GGX with `metallic` / `roughness`) instead of the classic diffuse look. Turned on by `Material::pbr`, `with_metallic`, `with_roughness`. Default false.
- `metallic`: 0 = dielectric (plastic, wood, stone), 1 = metal. Used when `pbr` is on. Out-of-range values are clamped (like `Material::with_metallic`, which also turns `pbr` on; writing the field alone does not). Range 0 to 1. Default 0.
- `roughness`: 0 = mirror-smooth, 1 = fully rough. Used when `pbr` is on. Out-of-range values are clamped (like `Material::with_roughness`, which also turns `pbr` on; writing the field alone does not). Range 0 to 1. Default 0.5.
- `emissive`: Light the surface emits by itself, added after lighting (black = none). Works in every mode. Default rgba(0, 0, 0, 1).
- `normal_map`: Tangent-space normal map (OpenGL convention: green points along +V). Works with and without `pbr`.
- `orm_map`: Packed surface data (used when `pbr` is on): R = ambient occlusion, G = roughness, B = metallic; each is multiplied with `roughness` / `metallic`. Like the normal map it holds data, not colors.
- `mass`: Total mass in kg. Assigning `nil` lets the physics engine compute it from the volume (density 1). Greater than 0.
- `sensor`: Sensors detect overlaps (collision events) but don't push anything. Default false.
- `gravity_scale`: Multiplier of the world gravity for this body (0 = floats, negative = rises). Default 1.
- `velocity`: World-space velocity (m/s). Written back by the simulation; set it to change it. Unit: m/s. Default vec3(0, 0, 0).
- `angular_velocity`: World-space angular velocity (rad/s). Unit: rad/s. Default vec3(0, 0, 0).
- `on`: events and timers owned by the object
- `play`: animation (glTF models)
- `add_body`: physics

### `Camera`

```luau
camera.position: vector
camera.x: number
camera.y: number
camera.z: number
camera.rotation: vector
camera.fov: number
camera.near: number
camera.far: number
camera.ortho: number?
camera:look_at(target: vector | Object)
camera:world_to_screen(pos: vector | Object) -> (number?, number?, number?)
camera:screen_to_ray(x: number?, y: number?) -> (vector, vector)
camera:translate(delta: vector)
camera:forward() -> vector
camera:right() -> vector
camera:up() -> vector
```

## Types

### `Key`

```luau
type Key = "a" | "b" | "c" | "d" | "e" | "f" | "g" | "h" | "i" | "j" | "k" | "l" | "m" | "n" | "o" | "p" | "q" | "r" | "s" | "t" | "u" | "v" | "w" | "x" | "y" | "z" | "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "space" | "enter" | "escape" | "tab" | "backspace" | "left" | "right" | "up" | "down" | "lshift" | "rshift" | "lctrl" | "rctrl" | "lalt" | "ralt" | "lsuper" | "rsuper" | "shift" | "ctrl" | "control" | "alt" | "super" | "meta" | "cmd" | "win" | "esc" | "return" | "f1" | "f2" | "f3" | "f4" | "f5" | "f6" | "f7" | "f8" | "f9" | "f10" | "f11" | "f12" | "f13" | "f14" | "f15" | "f16" | "f17" | "f18" | "f19" | "f20" | "f21" | "f22" | "f23" | "f24" | "minus" | "equal" | "bracketleft" | "bracketright" | "backslash" | "semicolon" | "quote" | "comma" | "period" | "slash" | "backquote" | "-" | "=" | "[" | "]" | "\\" | ";" | "'" | "," | "." | "/" | "`" | "insert" | "delete" | "home" | "end" | "pageup" | "pagedown" | "del" | "ins" | "pgup" | "pgdn" | "capslock" | "numlock" | "scrolllock" | "printscreen" | "pause" | "menu" | "numpad0" | "numpad1" | "numpad2" | "numpad3" | "numpad4" | "numpad5" | "numpad6" | "numpad7" | "numpad8" | "numpad9" | "numpad_add" | "numpad_subtract" | "numpad_multiply" | "numpad_divide" | "numpad_decimal"
```

Keys accepted by input.down / pressed / released / repeated and sent by key_pressed / key_released (physical US-layout positions). Aliases: shift ctrl alt super esc return - = [ ] \ ; ' , . / ` del ins pgup pgdn ...

### `MouseButton`

```luau
type MouseButton = "left" | "right" | "middle" | "back" | "forward"
```

### `PadButton`

```luau
type PadButton = "a" | "b" | "x" | "y" | "lb" | "rb" | "lt" | "rt" | "back" | "start" | "lstick" | "rstick" | "up" | "down" | "left" | "right"
```

### `PadAxis`

```luau
type PadAxis = "left_x" | "left_y" | "right_x" | "right_y" | "lt" | "rt"
```

### `ColorLike`

```luau
type ColorLike = string | number | Color | vector | { number }
```

"red", "#ff8800", "#f80", "#rrggbbaa", 0xff8800, Color.rgb(...), vec3(r, g, b) or {r, g, b}

### `Ease`

```luau
type Ease = "linear" | "in" | "out" | "in_out" | "smooth" | "in_quad" | "out_quad" | "in_out_quad" | "in_cubic" | "out_cubic" | "in_out_cubic" | "in_sine" | "out_sine" | "in_out_sine" | "in_expo" | "out_expo" | "in_back" | "out_back" | "out_elastic" | "in_bounce" | "out_bounce"
```

### `Align`

```luau
type Align = "top_left" | "top" | "top_right" | "left" | "center" | "right" | "bottom_left" | "bottom" | "bottom_right"
```

### `BodyType`

```luau
type BodyType = "dynamic" | "static" | "kinematic"
```

### `BlendMode`

```luau
type BlendMode = "auto" | "opaque" | "alpha" | "additive"
```

### `BodySpec`

```luau
type BodySpec = { type: BodyType?, shape: ("auto" | "box" | "sphere" | "capsule" | "cylinder")?, size: vector?, radius: number?, height: number?, mass: number?, friction: number?, bounciness: number?, sensor: boolean?, lock_rotation: boolean?, gravity_scale: number?, damping: number?, angular_damping: number?, ccd: boolean?, velocity: vector?, angular_velocity: vector?, }
```

### `LightSpec`

```luau
type LightSpec = { type: ("point" | "spot")?, color: ColorLike?, intensity: number?, range: number?, shadows: boolean?, angle: number?, softness: number?, position: vector?, rotation: vector?, parent: Object?, name: string?, }
```

### `SpawnProps`

```luau
type SpawnProps = { name: string?, visible: boolean?, position: vector?, rotation: vector?, scale: (number | vector)?, color: ColorLike?, data: { number }?, blend: BlendMode?, double_sided: boolean?, cast_shadow: boolean?, pbr: boolean?, metallic: number?, roughness: number?, emissive: ColorLike?, parent: Object?, body: (BodyType | boolean | BodySpec)?, light: LightSpec?, shader: Shader?, normal_map: (Texture | string | false)?, orm_map: (Texture | string | false)?, node: string?, lod: { LodLevel }?, }
```

### `LodLevel`

```luau
type LodLevel = { number | Mesh }
```

`{distance, mesh}`: from `distance` meters away the object draws `mesh`.

### `Hit`

```luau
type Hit = { object: Object, point: vector, normal: vector, distance: number }
```

### `CollisionInfo`

```luau
type CollisionInfo = { speed: number, sensor: boolean }
```

### `ListenerOptions`

```luau
type ListenerOptions = { priority: number?, owner: Object?, once: boolean? }
```

### `TweenOptions`

```luau
type TweenOptions = { ease: Ease?, delay: number?, loop: boolean?, yoyo: boolean?, unscaled: boolean?, done: (() -> ())?, owner: Object?, }
```

### `TextOptions`

```luau
type TextOptions = { size: number?, color: ColorLike?, align: Align?, font: Font?, width: number?, line_height: number?, shadow: (boolean | ColorLike)?, }
```

### `SoundOptions`

```luau
type SoundOptions = { volume: number?, pitch: number?, pan: number?, loop: boolean?, fade_in: number?, bus: (string | Bus)?, at: (vector | Object)?, range: number?, }
```

### `MeshOptions`

```luau
type MeshOptions = { tile: number?, cell: number? }
```

### `PlayOptions`

```luau
type PlayOptions = { loop: boolean?, speed: number?, fade: number?, restart: boolean? }
```

### `SkySpec`

```luau
type SkySpec = { top: ColorLike?, bottom: ColorLike?, cubemap: (string | false)?, blend: number?, lighting: number? }
```

### `BloomSettings`

```luau
type BloomSettings = { threshold: number?, knee: number?, strength: number?, radius: number? } | boolean
```

### `SsaoSettings`

```luau
type SsaoSettings = { radius: number?, samples: number?, strength: number?, bias: number? } | boolean
```

### `MotionBlurSettings`

```luau
type MotionBlurSettings = { strength: number?, samples: number? } | boolean
```

### `DofSettings`

```luau
type DofSettings = { focus: number?, aperture: number?, focal_range: number?, samples: number? } | boolean
```

### `ColorGradingSettings`

```luau
type ColorGradingSettings = { exposure: number?, contrast: number?, saturation: number?, temperature: number?, tint: number?, lift: (vector | { number })?, gamma: (vector | { number })?, gain: (vector | { number })?, } | boolean
```

### `ChromaticAberrationSettings`

```luau
type ChromaticAberrationSettings = { amount: number?, radial: boolean? } | boolean
```

### `FilmGrainSettings`

```luau
type FilmGrainSettings = { strength: number?, size: number? } | boolean
```

### `VignetteSettings`

```luau
type VignetteSettings = { strength: number?, inner: number?, outer: number?, color: ColorLike? } | boolean
```

### `WidgetId`

```luau
type WidgetId = string | number
```

