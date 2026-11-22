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
| A8b | Хвосты лицензий: сжать встраиваемый текст (1.8 МБ в каждом exe игры); `--licenses` у редактора; `include_str!` ломает `cargo publish` крейта `spark` | ⏳ | | |
| A9 | `deny.toml` + cargo-deny в CI (L4) | ✅ код / ⏳ проверка | `deny.toml`, job `deny` в `ci.yml` | локально cargo-deny не запускался (не установлен) — проверить на CI. Копилефт только как альтернатива (`r-efi` LGPL, `self_cell` GPL — используются по MIT/Apache) |
| A10 | `ASSETS.md` — происхождение ассетов (L3, J15) | ✅ / ⏳ владелец | `ASSETS.md` | **владельцу:** подтвердить происхождение `figure.glb`, `hit.wav`, `spawn.wav` или заменить на CC0 |
| A11 | Удалить `crates/spark-editor.zip` (J1) | ✅ | `git rm`, `*.zip` в `.gitignore` | не закоммичено |
| A12 | Логи ошибок в папку данных пользователя (B8) | ✅ / ⏳ e2e | `spark-core/src/paths.rs`, `lib.rs::fail`, `app.rs::report_fatal` | упакованные игры: `<user data>/SparkGames/<game>/logs/`; dev-прогоны — `error.log` рядом с `main.luau`. Сквозной тест на упакованной игре не делался |
| A14 | Текст лицензий встроен один раз (`spark::THIRD_PARTY_LICENSES`), doc-комментарий `fullscreen`/`max_fps` (B7) | ✅ | `spark/src/lib.rs`, `spark-cli/src/main.rs`, `app.rs` | |
| A13 | Промпты/STATE из `docs/dev` (J2, J3) | ⏸ | | решение владельца: это его рабочие файлы |

## Этап B — Безопасность и устойчивость

| ID | Задача | Статус |
|---|---|---|
| B-1 | FBX: проверки индексов, лимит inflate, fuzz-цели (S1, S12) | ⏳ |
| B-2 | Убрать `unsafe env::remove_var`, `forbid(unsafe_code)` (S5) | ⏳ |
| B-3 | Devtools-токен от редактора, игнор `--devtools` в упакованных играх (S2, S3) | ⏳ |
| B-4 | Luau `sandbox(true)` (S4) | ⏳ |
| B-5 | Вьюпорт редактора: `catch_unwind`/watchdog, Play отдельным процессом (S6) | ⏳ |
| B-6 | `ImageData`, `dist.rs` симлинки/`tar --`, установщики (S8–S10) | ⏳ |
| B-7 | minisign по умолчанию + проверка в `spark update` (S7) | ⏳ |
| B-8 | Golden на CI блокирующие, MSRV-джоба (J9) | ⏳ |

## Этапы C–G

Не начаты. См. [11-roadmap.md](11-roadmap.md).

## Журнал

| Дата | Что |
|---|---|
| 2026-11-22 | Полный разбор, папка `analysis/`. Базовые числа: fmt/clippy чисто, 205 тестов прошли, 3 пропущены |
| 2026-11-22 | Старт этапа A: агенты 1 (strict names) и 2 (лицензии) запущены параллельно |
| 2026-11-22 | Агенты 1 и 2 готовы (A1–A6, A8–A10). Удалены `crates/spark-editor.zip` и пустая `examples/starfall`. Запущен агент 3 (A7 сид, A12 логи, общий const лицензий, doc-комментарий `max_fps`) |
