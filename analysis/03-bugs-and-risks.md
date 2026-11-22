# 3. Баги и риски

Серьёзность: **C** критично, **H** высокая, **M** средняя, **L** низкая.
Пометки источника: **[проверено]** — воспроизведено при разборе, **[код]** — видно в коде, **[AUDIT]** — из аудита, сверено с кодом.

## 3.1 Корректность и «тихие» ошибки

| ID | Sev | Что | Как воспроизвести / где | Источник |
|---|---|---|---|---|
| B1 | **H** | Колбэк с опечаткой (`function Update`, `function fixedUpdate`, `function Render`) **молча не вызывается** | `spark check` на игре с `function Update(dt)` → `ok`, ошибок 0 | [проверено] |
| B2 | **H** | Подписка на несуществующее событие (`on("colision", ...)`) принимается молча | тот же прогон | [проверено] |
| B3 | M | Опечатка в глобале (`mth.sin`) → «attempt to index nil with 'sin'» без подсказки `math` | `spark check` | [проверено] |
| B4 | M | Неизвестное поле объекта (`cube.postion`) ловится, но без «did you mean 'position'», а traceback содержит внутренние кадры `__mlua_newindex:16`, `[C]: in ?` | `spark check --json` | [проверено] |
| B5 | M | `test.expect_text` сразу после `wait_until` видит прошлый кадр; `test.texts()` дублирует перекрытый текст; `restart()` в тестах невозможен | `testing.rs` | [AUDIT N10] |
| B6 | L | Headless/тесты не сеют `math.random` → тесты с рандомом не детерминированы | нет ни одного `randomseed`/`seed` в `spark-script/src` | [код] |
| B7 | L | Doc-комментарий `fullscreen` висит на `max_fps` | `crates/spark/src/app.rs:230` | [код] |
| B8 | M | `error.log` / `spark-crash.log` пишутся рядом со скриптом/exe; в упакованной игре и в Program Files не пишутся, ошибку игрок не увидит | `spark-script/src/lib.rs:205`, `spark/src/app.rs:283` | [код] |
| B9 | M | Headless рендерит только последний кадр: временные пост-эффекты на скриншоте неверны, GPU-ошибки середины прогона не видны | `spark/src/app.rs:512-547` | [код] |

## 3.2 Надёжность и безопасность

| ID | Sev | Что | Где | Источник |
|---|---|---|---|---|
| S1 | **H** | FBX-парсер индексирует без проверок: `verts[cp*3]`, `influences[cp]` → паника на битом файле; инфлейт zlib без лимита | `crates/spark-core/src/fbx.rs:1090`, `:1136` | [код] [AUDIT N6] |
| S2 | **H** | Devtools без аутентификации: игра шлёт токен (протокол v2), но **редактор токен не выдаёт и не проверяет**; любой локальный процесс может подключиться первым | `spark-editor` (нет `SPARK_DEVTOOLS_TOKEN`) | [код] [AUDIT S1] |
| S3 | M | `--devtools ADDR` работает в упакованных (выпущенных) играх и подключается к любому хосту | `spark/src/app.rs:818` | [AUDIT S2] |
| S4 | M | Luau без `sandbox(true)`: моды/чужие скрипты могут менять глобалы и метатаблицы встроенных библиотек | `spark-script/src/lib.rs:261` | [AUDIT K9c] |
| S5 | M | `unsafe { std::env::remove_var("WGPU_BACKEND") }` — UB при наличии других потоков (аудио, тесты) | `crates/spark-render/src/init.rs:576` | [код] |
| S6 | M | Редактор исполняет игровой Luau в своём процессе без `catch_unwind` и лимита → падение игры роняет редактор | `spark-editor/src/workspace/viewport.rs:257` | [код] |
| S7 | M | `spark update` берёт установщик и `.sha256` из того же релиза — это целостность, не подлинность; minisign опционален | `spark-cli/src/update.rs`, `install.*` | [AUDIT K7] |
| S8 | M | `spark build` следует симлинкам без проверки выхода из корня; `tar` вызывается без `--` | `spark-cli/src/dist.rs` | [AUDIT S3] |
| S9 | M | Установщики: `install.ps1` переименовывает старый exe без отката; `install.sh` может записать по подложенному симлинку | `install.ps1`, `install.sh` | [AUDIT S4] |
| S10 | M | `ImageData` с публичными полями, `width*height*4` переполняет `u32` | `spark-core/src/assets.rs` | [AUDIT N7] |
| S11 | L | `icon.rs`: предсказуемый временный путь, `dangling::<u16>()` как `MAKEINTRESOURCE` | `spark-cli/src/icon.rs` | [AUDIT S7] |
| S12 | L | Нет fuzz-целей (glTF, FBX, pack, devtools, save, изображения, аудио), нет miri/sanitizers | — | [AUDIT] |

## 3.3 Производительность (структурные пределы)

| ID | Sev | Что | Где |
|---|---|---|---|
| P1 | **H** | 10k динамических тел ≈ 36.6 мс CPU на кадр [проверено] — физическая синхронизация O(n) с аллокациями каждый шаг, статические тела проверяются тоже | `spark-physics/src/lib.rs:174-259` |
| P2 | H | CPU-скининг: вершинный буфер каждой анимированной модели перезаливается каждый кадр; нормали скинятся без inverse-transpose | `spark-core/src/animation.rs`, [AUDIT R3] |
| P3 | M | 64 объекта на uniform-окно draw call; каждый пиксель перебирает до 64 источников света (нет кластеров) | `spark-render/src/consts.rs:16`, `surface.wgsl:139,248` |
| P4 | M | Нет depth pre-pass, непрозрачные сортируются по состоянию, а не front-to-back | [AUDIT R5] |
| P5 | M | Аллокации каждый кадр: bind groups пост-проходов, векторы света/теней, клоны анимаций | [AUDIT R7] |
| P6 | M | Каждый вызов API из Luau = `RefCell::borrow_mut` всего `World`; 10k `o:rotate_y` ≈ 4 мс | `spark-script/src/convert.rs` (`with`) |
| P7 | M | Hot reload перегружает все ассеты | `world.rs:111` |
| P8 | L | `Scene::find` линейный | `scene.rs:397` |

## 3.4 Рендер-баги

| ID | Sev | Что | Источник |
|---|---|---|---|
| R1 | H | Normal/ORM-карты загружаются как sRGB и переводятся обратно в шейдере; мипы усредняются в неверном пространстве → искажённые нормали и шероховатость вдали | [AUDIT R4] |
| R2 | M | `bloom.wgsl` дрожит со временем (шиммер) | [AUDIT R7] |
| R3 | M | glTF: игнорируются metallic-roughness/occlusion/emissive-текстуры, `alphaCutoff` модели, vertex colors, UV1, tangents, morph targets, KHR-расширения | [AUDIT R6] |
| R4 | L | IBL — box-filtered LDR мипы без BRDF LUT; швы кубических теней | [AUDIT R8] |

## 3.5 Рекомендуемый порядок закрытия

1. B1, B2, B3, B4 — один PR «strict names» (дни работы, огромный эффект для ИИ).
2. S1 + fuzz-цель FBX; S5; S6 (`catch_unwind` + вынос игры из процесса редактора или хотя бы изоляция).
3. S2 + S3 (токен и отключение devtools в упакованных играх).
4. B8 (логи в папку данных пользователя), B6 (сид), B9 (флаг `--render-every-frame` по умолчанию в `spark screenshot`).
5. P1 (dirty-флаги тел, кэш мировых матриц, без аллокаций), R1 (линейные data-текстуры).
