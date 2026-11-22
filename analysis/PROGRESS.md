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

## Этапы C–G

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
