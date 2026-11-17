# Промпт для продолжения (скопируй целиком в новую сессию)

Ты работаешь над движком Spark (Rust + wgpu + Luau) в `C:\Users\Dimsk\Documents\GitHub\Spark`, ветка `overhaul/phase-0-1` (форк `origin` = dimskoff/Spark; `main` не трогать, force-push нельзя). Это продолжение большой переделки. **Сначала прочитай, не пиши код:** `STATE.md`, `docs/ROADMAP.md`, `docs/audit/AUDIT.md`, `docs/audit/CLEANUP.md`, `docs/adr/*`, `CHANGELOG.md` (раздел Unreleased), `git log --oneline -20`. Затем запусти и запомни базовые числа:

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
Всё должно быть зелёным. Правила: ничего не «готово» без запуска; не отключать тесты и линты (локальный `#[allow]` только с причиной); тесты запускай **редко и разом** (экономь лимиты); новое поведение — сначала падающий тест; публичный Luau API / `game.toml` менять только с записью в CHANGELOG; код и доки на английском, общение со мной на русском; файлы игрового кода открывать только через `spark_core::vfs`, числа из Luau принимать через `convert::Num/to_num`.

## Что уже сделано (проверено запуском)
- Фаза 0: аудит и роадмап. Фаза 1a: NaN/inf-валидация, path jail, durable `save`, лимиты pack/devtools, строгий CI (fmt + clippy -D), примеры, golden.
- Реестр API: `crates/spark-api` парсит `docs/spark.d.luau` → `docs/API_REFERENCE.md`, `docs/api.json`, `llms.txt`, `llms-full.txt`; дрейф ловят `spark-api/tests/generated.rs` и `spark-script/tests/api_drift.rs`. Перегенерация: `SPARK_BLESS=1 cargo test -p spark-api`.
- CLI: `spark api|doctor|screenshot|inspect|golden|diff|mcp`, `--json` у new/docs/build/update/check/test, коды выхода 0/1/2.
- Диапазоны (fov, near/far, размеры мешей, mass/friction), лимит памяти Luau, alpha-cutoff по режимам смешивания.
- Devtools протокол v2 (`SetObject`, `Hello.token`) — **со стороны игры сделано, редактор его ещё не использует** (токен не выдаётся, `SetObject` не отправляется).
- Релиз: workflow с `SHA256SUMS`, опциональной minisign-подписью, установщики берутся из релиза, `docs/RELEASING.md`.

## Что осталось (в порядке приоритета; по каждому — анализ, затем реализация)
1. **Проверить CI на GitHub** (Linux/macOS не запускались локально): прогнать, починить красное. Golden-шаг пока `continue-on-error` (картинки сняты на GTX 1660; на lavapipe отличаются) — перегенерировать эталоны на CI (`spark golden <dir> --update`) и сделать шаг блокирующим.
2. **Редактор** — отдельный промпт `docs/EDITOR_PROMPT.md` (самая большая задача).
3. **Devtools**: выдавать токен из редактора (env `SPARK_DEVTOOLS_TOKEN`, проверка в `Hello`), игнорировать `--devtools` в упакованных играх; затем расширить MCP: `spark_attach` (пауза/шаг/`SetObject`/снимок сцены живой игры), `spark_eval` (Luau в запущенной игре, отключаемый), точки останова, лог «чёрного ящика».
4. **Агентский опыт** (см. раздел 8 AUDIT): строгие глобалы и «did you mean» для опечаток в функциях/полях/колбэках/событиях; коды ошибок `SPK-xxxx`; JSON ошибок разделить на `message/hint/traceback` без внутренних кадров; линтер типичных ошибок ИИ; `test.click_ui(id)`, `test.restart()`, дедупликация `test.texts()`, посев `math.random` в headless/test, `spark fix-suggest`, `spark record`, `spark profile`, `spark bench`; бенчмарк «агент с нуля» (15–20 заданий, KPI ≥80% за ≤3 итерации).
5. **Безопасность**: `cargo-fuzz` для glTF/FBX/pack/devtools/save/изображений; починить FBX-паники (`fbx.rs`: индексы control points, overflow `(ni+1)*3`, zlib-лимит); инварианты `ImageData`; `lua.sandbox(true)` (ADR 0002: нужен редизайн установки глобалов/модулей); подпись релизов (нужен ключ владельца, `docs/RELEASING.md`) + проверка в `spark update`; `dist.rs` симлинки, `tar --`; убрать `unsafe env::remove_var` в `spark-render/init.rs`; crash handler с файлом отчёта; `cargo deny`.
6. **Рендер (Renderer 2.0)**: нормали/ORM грузятся как sRGB (R4 в AUDIT) — линейные data-текстуры; CSM-тени со стабилизацией; GPU-скининг и morph targets; полный glTF PBR (ORM/occlusion/emissive-текстуры, alphaCutoff, vertex colors, UV1, tangents); tonemapping по умолчанию; mip-bloom; TAA; IBL с HDR; затем render graph, storage-buffer инстансинг, clustered lighting, GI/атмосфера/частицы/террейн. Каждый шаг — golden-скриншот + замер (benchmark-сцены 1k/10k/100k объектов; таблица профилей).
7. **Ядро**: формат сцен/префабов с миграциями, GUID ассетов и кэш импорта, асинхронная загрузка, анимационные state machine, retained UI, action-mapping, навигация, сеть, локализация (по ADR на каждое).
8. **Доки**: mdBook-сайт, исполняемые примеры в доках, проверка ссылок, матрица функций, `ARCHITECTURE.md`, `docs/MIGRATION.md`, `CODE_OF_CONDUCT.md`, README (EN/RU) под реальность (упомянуть новые команды и `examples/`).
9. Убрать из репозитория то, что устарело (см. CLEANUP.md): решить судьбу `examples/demo` и `examples/hello`, чужие ветки на `origin` — только список владельцу, не удалять.

## Как анализировать перед каждым пунктом
Для пункта: (1) найди в коде реальные точки (`Grep`), (2) запусти текущее поведение и зафиксируй вывод, (3) напиши падающий тест, (4) реализуй, (5) один общий прогон fmt/clippy/test, (6) запись в CHANGELOG/AUDIT/STATE, (7) коммит маленькими осмысленными коммитами и `git push` в ветку (не в main). В конце сессии обнови `STATE.md` (что сделано / в процессе / команды) и дай короткий отчёт с реальными числами. Если лимит заканчивается — сначала запиши состояние в `STATE.md`, потом коммит.
