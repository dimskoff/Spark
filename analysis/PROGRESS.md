# Прогресс по роадмапу

Единый журнал: что сделано, что в работе, что осталось. Обновляется после каждой задачи.
План и обоснования — в [11-roadmap.md](11-roadmap.md), баги — в [03-bugs-and-risks.md](03-bugs-and-risks.md).
ID задач (B1, S2, L1, J1 …) совпадают с ID в файлах разбора.

Обозначения: ✅ сделано и проверено тестом · 🔄 в работе · ⏳ не начато · ⏸ отложено (с причиной)

Ветка: `overhaul/phase-0-1`. Базовая точка разбора: `9269188`.

## Этап A — «Ноль тихих ошибок» + юридическая гигиена (версия 0.2)

| ID | Задача | Статус | Где / коммит | Заметки |
|---|---|---|---|---|
| A1 | Строгие глобалы + did-you-mean (`mth` → `math`) (B3) | ✅ | `spark-script/src/names.rs`, `tests/strict.rs` | метатаблица на `_G`; движок читает колбэки через `raw_get` |
| A2 | Проверка имён колбэков (`Update`, `fixedUpdate`, `Render`…) (B1) | ✅ | `names.rs::misspelled_callback` | `updateScore`, `render_ui`, `restart` не флагуются |
| A3 | Неизвестные встроенные события в `on()` (B2) | ✅ | `names.rs::event_error`, `flow.rs` | on/once/obj:on/obj:once |
| A4 | did-you-mean по полям + чистый traceback (B4) | ✅ частично | `types.rs` (`OBJ_MEMBERS`), `names.rs::clean_error` | только `Object`; `Cam`, `LightRef` и др. — ⏳; список полей ручной (не сверяется с регистрацией) |
| A5 | Коды ошибок `SPK-xxxx` в `check --json` | ✅ | `names.rs::classify`, `spark-cli/src/check.rs`, «Error codes» в `docs/SPARK_API.md` | SPK-1000..1006, 1100 (синтаксис), 1101 (типы); поле `hint` |
| A6 | Подсказка `task.spawn` → `task(fn)` | ✅ | `lib.rs` | |
| A6b | Хвосты: did-you-mean для остальных userdata; `parse_error` берёт строку из traceback (сообщение `on` искажается); e2e-тест SPK-1006 | ⏳ | | найдено агентом 1 |
| A7 | Сид `math.random` в headless/test, `--seed` (B6) | ✅ | `app.rs` (`RunArgs::effective_seed`), `ScriptGame::seed`, `tests/seed.rs` | явный `--seed` > 0 в headless/check/test > случайно в окне; пересев при reload/restart; u64 сворачивается в 32 бита (`low ^ high`) |
| A8 | `THIRD_PARTY_LICENSES` в релизе, `spark build`, `spark licenses`, `game --licenses` (L1, L2, L6) | ✅ | `tools/licenses` (генератор из `cargo metadata`), `THIRD_PARTY_LICENSES.md`, `dist.rs`, `app.rs`, `release.yml` | 525 крейтов + Noto Sans, 222 группы текстов, 1.8 МБ; тест на устаревание; у 69 крейтов нет файла лицензии — текст восстановлен из SPDX + `authors` (помечено в файле) |
| A8b | Хвосты лицензий: `--licenses` у редактора; `include_str!` ломает `cargo publish` крейта `spark` (размер решён в A16) | ⏳ | | |
| A9 | `deny.toml` + cargo-deny в CI (L4) | ✅ код / ⏳ проверка | `deny.toml`, job `deny` в `ci.yml` | локально cargo-deny не запускался (не установлен) — проверить на CI. Копилефт только как альтернатива (`r-efi` LGPL, `self_cell` GPL — используются по MIT/Apache) |
| A10 | `ASSETS.md` — происхождение ассетов (L3, J15) | ✅ | `ASSETS.md` | |
| A15 | Удалить `figure.glb`, `hit.wav`, `spawn.wav` (владелец: мусор от другой игры, новых ассетов не добавлять) | ✅ | `crates/spark-script/tests/common/mod.rs::write_test_rig` (GLB генерируется во временную папку теста), `flow.rs`, `api_drift.rs`, `models.luau`; звук убран из `examples/demo` | новых медиафайлов в репо нет. Файлы остаются в истории git — вычистить только по решению владельца |
| A16 | Лицензии компактно: список «крейт — версия — лицензия — ссылка — ©» + по одному тексту каждой лицензии (владелец: 1.8 МБ — перебор) | ✅ | `tools/licenses/src/main.rs`, `THIRD_PARTY_LICENSES.md` 104 КБ; `const` → `static` в `spark/src/app.rs` (была двойная копия) | 472 строки на 525 крейтов, 12 текстов лицензий; для `A OR B` берётся MIT, потом Apache-2.0. `spark.exe` 21.4 МБ (до лицензий 21.2) |
| A11 | Удалить `crates/spark-editor.zip` (J1) | ✅ | `git rm`, `*.zip` в `.gitignore` | не закоммичено |
| A12 | Логи ошибок в папку данных пользователя (B8) | ✅ / ⏳ e2e | `spark-core/src/paths.rs`, `lib.rs::fail`, `app.rs::report_fatal` | упакованные игры: `<user data>/SparkGames/<game>/logs/`; dev-прогоны — `error.log` рядом с `main.luau`. Сквозной тест на упакованной игре не делался |
| A14 | Текст лицензий встроен один раз (`spark::THIRD_PARTY_LICENSES`), doc-комментарий `fullscreen`/`max_fps` (B7) | ✅ | `spark/src/lib.rs`, `spark-cli/src/main.rs`, `app.rs` | |
| A13 | Промпты/STATE из `docs/dev` (J2, J3) | ⏸ | | решение владельца: это его рабочие файлы |

## Этап B — Безопасность и устойчивость

| ID | Задача | Статус | Кто |
|---|---|---|---|
| B-1 | FBX: проверки индексов, лимит inflate, мутационные тесты, скелет `fuzz/` (S1, S12); ревизия glTF | ✅ | `fbx.rs`, `model.rs` (`ModelData::load_bytes`, `catch_unwind` вокруг крейта `gltf`), `spark-core/tests/fbx_robustness.rs` (7000 мутантов, 0 паник), `fuzz/` (не запускался) |
| B-1b | Хвосты: крейт `gltf` сам паникует на битых accessor'ах (сейчас ловим `catch_unwind`) — нужна своя проверка до вызова; линейный поиск связей FBX O(n²); лимиты декодирования текстур `image` | ⏳ | |
| B-2 | Убрать `unsafe env::remove_var`, `forbid(unsafe_code)`, `icon.rs` (S5, S11) | ✅ | `spark-render/src/init.rs::backends_from_value`; `forbid(unsafe_code)` в 10 крейтах; unsafe остался только Win32 (icon.rs, app.rs MessageBox, editor os.rs) с SAFETY |
| B-2b | Атрибуты в корни `spark-script` (forbid), `spark` и `spark-editor` (deny + allow на модуль) | ✅ | `#[allow(unsafe_code)]` только на `app.rs::report_fatal` и `editor mod os` |
| B-3 | Devtools-токен через `SPARK_DEVTOOLS_TOKEN`, только loopback, игнор в упакованных играх (S2, S3) | ✅ код (ждёт общий прогон) | `spark-devtools` (протокол 3, мин. 2; `Server::poll_game`, 8 плохих попыток), `spark-editor/src/process.rs`, `spark/src/{app,devtools}.rs` (`vfs::is_packed`) |
| B-4 | Luau sandbox: встроенные библиотеки и API только для чтения, SPK-1007 (S4) | ✅ | `lua.sandbox(true)`, свой env пользователя; `lib.rs::chunk()` — все чанки (main, модули, тест) грузятся с префиксом `if false then name=nil … end` для всех не-встроенных идентификаторов, чтобы Luau не кэшировал пользовательские глобалы; `tests/sandbox.rs` (17 тестов); ADR 0002 обновлён. Скорость `math.*`: 4.7 с → 1.44 с |
| B-4b | Тех. долг sandbox: префикс мёртвого кода — хак; `loadstring` компилируется без него (возможны устаревшие значения); затенение встроенного имени не видно уже загруженным модулям; SPK-1007 не называет таблицу; политика модов (этап 7) | ⏳ | найдено на общем прогоне: без исправления тест видел `opened = false` после `opened = true` |
| B-5 | Вьюпорт редактора: `catch_unwind` + watchdog 3 с (S6); правка Transform/visible в инспекторе через `SetObject` (E1, без undo) | ✅ код / ⏳ ручная проверка в редакторе | `viewport.rs::run_guarded`, `workspace/{mod,game}.rs`, локали `viewport.panicked*` |
| B-6 | `ImageData`, `dist.rs` симлинки/`tar --`, установщики (S4 установщики, S8, S10) | ✅ / ⏳ install.sh не прогонялся (нет bash) | `assets.rs` (`try_new/validate`, 16384, checked), `dist.rs`, `install.ps1` (проверка + откат), `install.sh` (`mktemp`, атомарный `mv`) |
| B-7 | minisign по умолчанию + проверка в `spark update` (S7) | ⏸ | нужен ключ владельца. Сейчас: `spark update` minisign не проверяет вообще, только SHA-256 из того же релиза (ловит порчу, не подмену) |
| B-8 | Golden на CI блокирующие, MSRV-джоба (J9) | ⏸ | нужен прогон CI на GitHub (push) |

## Этап C — Сцены, реестр типов, команды (версия 0.3)

План шагов — в [stage-c-plan.md](stage-c-plan.md) (после ADR).

| ID | Задача | Статус | Где | Заметки |
|---|---|---|---|---|
| C-ADR | 3 ADR: формат сцены, реестр типов, компонентная модель + команды | ✅ | `docs/dev/adr/0003..0005`, `stage-c-plan.md` | TOML, свой `spark-reflect` + derive, узел + закрытый набор компонентов (не ECS), `Command` + `History` в `spark-core` |
| C-1, C-2 | `spark-reflect` + derive | ✅ | `crates/spark-reflect`, `crates/spark-reflect-derive`, `def5137` | 18 тестов + 19 compile-fail (trybuild); сборка с нуля 13.8 с; отклонения от ADR записаны в ADR 0004. Нет: glam-типов (решить в C-3), `u64/usize` |
| C-4 | `Object.transform` (механическая замена) | ✅ | `spark-core/src/scene.rs` (`Transform`, `IDENTITY`), 11 файлов, `4bf38f1` | 288 тестов, все примеры `check` ok; Luau API не менялся |
| C-3, C-5 | derive на типах ядра + реестр компонентов | ✅ | `a886838`, `19257ae`; `spark-core/src/{reflect,registry}.rs`, `spark-reflect/src/glam_impls.rs` | 335 тестов; exe +512 байт. Поворот в файлах — Эйлер в градусах (Y-X-Z). Хэндлы (texture/shader/mesh/lod) — `transient` до C-11. Побочный эффект `with_metallic → pbr=true` и алиасы blend остаются в Luau-слое (C-8). `Shape::Box{size}/Sphere{radius}`. `LuaScope::{Object, Handle}` для `obj.light.*` и `camera.*` |
| C-6, C-7 | did-you-mean из реестра, сгенерированные биндинги transform/name/visible | ✅ | `1ccc271`, `a39bf97`; `spark-script/src/reflect_bind.rs`, `tests/obj_members.rs`, `tests/bind_bench.rs` (#[ignore]) | типизированные аксессоры по пути реестра (без `Value` на горячем пути); бенчмарк: разница в пределах шума (позиция 5.8 → 6.4 мс/кадр на 10k, контрольный `x` плавает так же). Вручную пока: `x/y/z`, материал, body, light, mesh, parent. `OBJ_EXTRA_FIELDS`: id, x, y, z, material, parent, body, light |
| C-11 | рецепты мешей | ✅ | `086ced4`; `spark-core/src/mesh_source.rs`, `tests/mesh_recipes.rs` | cube/box/plane/quad/sphere/cylinder/triangles + asset (путь + узел + primitive); `Mesh.custom` → runtime. `triangles` хранит точки в файле; skinned — в bind-позе |
| C-8, C-9 | биндинги material, body, light; удалить `PROPS` | ✅ | `0cc9d49`, `7f0ec30`; `reflect_bind.rs`, `tests/bind_errors.rs` (65 снимков текстов ошибок) | побочные эффекты (`metallic → pbr`, алиасы blend) сохранены; изменился только текст ошибки неизвестного ключа в `spawn{}` (did-you-mean + порядок allowed). Вручную: хэндлы (material, shader, normal_map, orm_map, mesh, lod, body, light, parent), `x/y/z`, `light.angle/softness` (поля spot-варианта). Бенчмарк: в пределах шума |
| C-12 | `NodeId`, `Command`, `World::apply`, `History` | ✅ | `4b0d108`; `spark-core/src/{node_id,command,history}.rs`, `tests/commands.rs` (14) | id ленивые, 8 hex (детерминированный генератор); доп. вариант `Restore` (точные типизированные состояния для обратных команд — JSON-кодирование решить в C-13); слияние SetProperty до `seal()`. Undo удаления не восстанавливает: аниматор, таймеры/слушатели, порядок корней; `Mesh.custom` — только пока жив хэндл. `History::clear` на reset/hot reload ещё не подключён |
| C-10 | `spark.d.luau`/`api.json` из реестра | ✅ / ⏳ luau-lsp | `ea087bc`; `spark-reflect/src/luau_decl.rs`, `spark-api/src/components.rs` | 3 блока `-- @generated components` (спеки, `LightRef`, поля `Object`); 88 пар `имя: тип` совпали один в один. Вручную: методы, `Camera`, `id/x/y/z/material/parent/light/body`, часть ключей spawn. В `api.json` секция `components`. luau-lsp не установлен — `check --types` не прогонялся |
| C-13 | devtools протокол 4 (команды) | ✅ | `a81ae72`; `spark-devtools/src/command_json.rs`, `spark/src/devtools.rs` (`CommandHost`) | `{"op":"set","node":..,"path":"body.mass","value":80}`; ошибки с кодом и подсказками; обратные команды без JSON-формы (`Restore`, отмена `Delete`) хранятся в игре (256 шт.) и отдаются токеном `{"op":"restore","token":N}`. `SetObject` — поверх `Batch`. Сквозного теста с живой игрой нет (нет headless-обвязки) |
| C-14 | `SceneDoc` + крейт `spark-scene`, `.scene` v1, миграции | 🔄 | | агент |
| C-15..C-18 | см. `stage-c-plan.md` | ⏳ | | |
| R-flaky | Зависание `spark_render` в полном `cargo test --workspace` | ✅ обход / ⏳ причина | `d4fd5b3`, `spark-render/src/tests.rs::gpu_guard` | воспроизведено: 4 параллельных процесса × 16 потоков, ~2% прогонов; все 9 GPU-тестов застревают на создании устройства wgpu (не readback). Обход: GPU-тесты по одному на процесс + сторож 90 с (падение с именем теста вместо вечного зависания). Корень (драйвер/wgpu при параллельном создании устройств) без стека не найден |

Открытые вопросы владельцу (из ADR): камера — синглтон, переезд глобала `camera` на узел (C-17); `Mesh.custom` не сохраняется (save сообщит, не молча); канонический писатель теряет комментарии в `.scene` (потом `toml_edit`); пропуск значений по умолчанию → смена дефолта = миграция; префабы v1 без удаления узлов и вложенных overrides; порядок скриптов объектов — после глобального `update`, по порядку создания.

## Этапы D–G

Не начаты. См. [11-roadmap.md](11-roadmap.md).

## Журнал

| Дата | Что |
|---|---|
| 2026-11-22 | Полный разбор, папка `analysis/`. Базовые числа: fmt/clippy чисто, 205 тестов прошли, 3 пропущены |
| 2026-11-22 | Старт этапа A: агенты 1 (strict names) и 2 (лицензии) запущены параллельно |
| 2026-11-22 | Агенты 1 и 2 готовы (A1–A6, A8–A10). Удалены `crates/spark-editor.zip` и пустая `examples/starfall`. Запущен агент 3 (A7 сид, A12 логи, общий const лицензий, doc-комментарий `max_fps`) |
| 2026-11-22 | Агент 3 готов. Общая проверка: fmt/clippy чисто, **233 теста прошли**, 0 упало, 3 пропущены; все примеры и шаблон `spark new` проходят `check`/`test`. Замер: строгие глобалы без потерь (цикл `math.*` 64.7 мс глобал vs 64.9 мс локальный алиас); 10k тел 30 мс/кадр, 10k кубов 3.9 мс. `spark.exe` release 23.1 МБ (было 21.2, +1.9 МБ — текст лицензий). Коммит `6509e0d`. Запущен агент 4 (A15) |
| 2026-11-22 | A15 (чужие ассеты удалены, коммит `3d81d44`), A16 (лицензии 1.8 МБ → 104 КБ, exe 21.4 МБ). **Этап A закрыт**, кроме хвостов A4/A6b, A8b, A9 (проверка cargo-deny на CI). Старт этапа B |
| 2026-11-22 | **Этап B закрыт** (B-1..B-6; B-7 и B-8 ждут владельца/CI). Общий прогон нашёл баг sandbox (устаревшие глобалы между чанками) — исправлен. Итог: fmt/clippy чисто, **288 тестов**, 0 упало, 3 пропущены; все примеры проходят; `spark.exe` 21.5 МБ |
| 2026-11-22 | Ветка `overhaul/phase-0-1` запушена (`7864e9b`). CI на неё **не запустился**: `ci.yml` срабатывает только на push в `main` и на pull request — для A9/B-8 нужен PR (`gh` локально не установлен). Старт этапа C: агент пишет ADR |
| 2026-11-22 | Этап C: ADR (`d1533af`), C-1/C-2 (`def5137`), C-4 (`4bf38f1`), C-3/C-5, C-6/C-7, C-11. Общий прогон: fmt/clippy чисто, **344 теста**, 0 упало, 4 пропущены (3 + бенчмарк); все 10 примеров `check` ok. Агенты C-6/C-7 и C-11 останавливались на лимите API и были продолжены |
| 2026-11-22 | C-8/C-9, C-12. Общий прогон: fmt/clippy чисто, **366 тестов**, 0 упало, 6 пропущены (3 + 3 бенчмарка); 10 примеров `check` ok |
| 2026-11-22 | C-10, C-13. Общий прогон: **383 теста**, 0 упало, 6 пропущены; 10 примеров ok. ⚠ Дважды за день тестовый бинарник `spark_render` зависал при полном `cargo test --workspace` (в одиночку проходит за 3 с) — причина не найдена, похоже на гонку за GPU-устройство между параллельными тестами; задача R-flaky ниже |
