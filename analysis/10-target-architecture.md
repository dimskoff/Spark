# 10. Целевая архитектура

Принцип: **лёгкий и простой снаружи, строгий внутри.** Не превращаться в Bevy (ECS ради ECS) и не тащить вес Unity.
Нужны четыре фундаментальных слоя, на которых строится всё остальное.

## 10.1 Слой 1 — реестр типов (reflection)

Один реестр в Rust описывает каждый компонент, его поля, типы, диапазоны, значения по умолчанию, документацию:

```rust
#[derive(Component, Reflect)]
#[doc = "Rigid body simulated by physics"]
pub struct Body {
    #[reflect(default = "dynamic")] pub kind: BodyKind,
    #[reflect(range = 0.0..=1e6, unit = "kg")] pub mass: Option<f32>,
    #[reflect(range = 0.0..=1.0)] pub bounciness: f32,
    ...
}
```

Из реестра **генерируются**: Luau-биндинги (геттеры/сеттеры с проверкой диапазонов и did-you-mean), `spark.d.luau`,
`SPARK_API.md`, `api.json`, `llms.txt`, инспектор редактора, сериализация сцены, схема devtools/MCP.
Сегодня это пять ручных копий (гл. 2, A3-A4) — после переворота источника правды расхождения исчезнут физически.

Реализация: свой лёгкий derive-макрос (`spark-reflect`) или `bevy_reflect` как зависимость (MIT/Apache, стабилен) —
решение оформить ADR.

## 10.2 Слой 2 — сцена как документ

**Объект = узел + набор компонентов** (не ECS-хранилище, а «Godot-like» узлы с компонентами — проще людям и ИИ):

```toml
# levels/1.scene  (spark scene format v1)
[[node]]
id = "a1f3"           # стабильный id для ссылок и диффов
name = "Player"
transform = { position = [0, 1, 0], rotation_deg = [0, 90, 0] }
mesh = "assets/hero.glb#Body"
material = { preset = "pbr", color = "#d8a45a", roughness = 0.6 }
body = { kind = "dynamic", shape = "capsule", radius = 0.4, height = 1.8 }
script = "src/player.luau"

[[node]]
id = "b7c2"
parent = "a1f3"
prefab = "prefabs/sword.prefab"
overrides = { "transform.position" = [0.3, 0, 0] }
```

- Текстовый, построчный, удобный для git-диффов и для ИИ (TOML или RON — ADR).
- Версия формата + миграции (`spark migrate`).
- Префабы = сцены с overrides.
- Luau: `scene.load("levels/1.scene")`, `spawn("prefabs/enemy.prefab", {position=...})`, `scene.save(...)` для редактора.
- **Код-первый путь остаётся**: всё, что делает сцена, можно сделать из Luau. Сцена — опция, не обязанность.

## 10.3 Слой 3 — команды

Все изменения документа сцены — команды: `Spawn`, `Delete`, `SetProperty{node, path, value}`, `Reparent`, `AddComponent`…
- Undo/redo в редакторе.
- Тот же набор — в devtools-протоколе (живая игра) и MCP (агент).
- Журнал команд агента с откатом каждой операции.

## 10.4 Слой 4 — ассеты

- GUID в соседнем файле `model.glb.meta` (TOML: guid, настройки импорта).
- Этап импорта (`spark-import` крейт, вне рантайма): glTF/FBX/OBJ → внутренний формат, мипы, тангенты, сжатие KTX2,
  лайтмап-UV; кэш в `.spark/cache/` (в `.gitignore`).
- Асинхронная загрузка с хэндлами (`Model.load_async` → `asset:ready()`), hot reload **одного ассета** без перезапуска игры.
- Упаковка `spark build` берёт импортированные ассеты.

## 10.5 Скрипты на объектах

```lua
-- src/door.luau
local Door = {}
Door.props = { open_angle = 90, speed = 2 }   -- видно в инспекторе
function Door:start() self.closed = self.rotation end
function Door:update(dt) ... end
function Door:on_collision(other) ... end
return Door
```

- Поля `props` попадают в инспектор (тип выводится из значения или из Luau-аннотации).
- Ошибка в скрипте одного объекта **выключает только этот объект** (с пометкой в редакторе), а не всю игру (гл. 2, A6).
- Глобальный `main.luau` с `start/update` остаётся для простых игр.

## 10.6 Рантайм: изменения без переписывания

| Сейчас | Цель |
|---|---|
| `Rc<RefCell<World>>` + swap вокруг каждого вызова | Оставить (это работает), но добавить пакетные API (`objects:rotate_y(dt)` на списке) и кэш свойств |
| Физика синхронизирует все тела каждый шаг | Dirty-флаги на теле/трансформе, кэш мировых матриц, без аллокаций |
| Полный перезапуск при сохранении | Hot reload модуля: перезагрузка только изменённого модуля + `on_reload(old_state)`; ассеты не трогать |
| Ошибки — строки | `SparkError { code: "SPK-2003", message, hint, location, docs_url }` |
| Глобальный Luau без sandbox | `sandbox(true)` + read-only встроенные таблицы + strict globals |

## 10.7 Рендер

```
RenderGraph
 ├─ ShadowPass (CSM ×4, point/spot atlas)
 ├─ DepthPrepass → HZB (позже)
 ├─ ClusteredLightCull (compute)
 ├─ Opaque (storage-buffer instancing; uniform fallback для GL)
 ├─ Sky / Atmosphere
 ├─ Transparent + Particles
 ├─ PostFX: TAA → Bloom(mip) → DoF → Tonemap(AgX/ACES) → FXAA(опц.) → Grade
 └─ Canvas2D / UI
```
Профилировщик: timestamp-запросы на каждый проход → `spark profile`, оверлей F1, devtools.

## 10.8 Редактор

- Процесс редактора: документы (сцены, скрипты), команды, undo, вьюпорт **сцены** (рендер того же `spark-render`).
- Процесс игры: Play запускает `spark` отдельно с `--devtools` + токеном; live-правки шлются командами протокола.
- Разбить `workspace/mod.rs`: `layout`, `commands`, `dialogs`, `fs_ops`, `panels/*`.
- Панели строятся из реестра (инспектор), а не руками.

## 10.9 Слои крейтов (цель)

```
spark-reflect   ← derive + реестр
spark-core      ← данные мира, сцена-документ, команды (без GPU, без парсеров)
spark-import    ← glTF/FBX/изображения/аудио → кэш (инструмент, не в рантайме игры)
spark-render / spark-physics / spark-audio / spark-window  ← бэкенды
spark-script    ← биндинги, генерируемые из реестра
spark           ← App, цикл
spark-devtools  ← протокол = сериализованные команды
spark-cli / spark-editor / spark-mcp  ← инструменты
плагины (опц.): spark-discord, spark-net, spark-nav, spark-particles
```
