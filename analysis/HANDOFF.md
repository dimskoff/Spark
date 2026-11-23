# Как продолжить работу (точка остановки)

Обновлено: 2026-11-22. Ветка `overhaul/phase-0-1` (локально впереди origin; последний пуш — `7864e9b`, конец этапа B).

## Где мы
- **Этап A** (ноль тихих ошибок, лицензии) — закрыт.
- **Этап B** (безопасность) — закрыт; B-7 (подпись minisign) ждёт ключ владельца, B-8 — прогон CI.
- **Этап C** (реестр типов, сцены, префабы, команды, скрипты на объектах) — **закрыт полностью** (C-1…C-18, последний коммит `4c3b9a7`). Итог: 482 теста, 0 падений, 11 примеров проходят `check`, golden совпадает.
- Полный журнал задач, коммитов, чисел тестов и решений — [PROGRESS.md](PROGRESS.md). План этапа C — [stage-c-plan.md](stage-c-plan.md). Решения — `docs/dev/adr/0003`…`0005` (+ разделы «C-xx decisions» в конце).

## Как проверить, что всё зелёное
```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace -j 4
cargo build -p spark-cli; Get-ChildItem examples -Recurse -Filter main.luau | % { target\debug\spark.exe check $_.DirectoryName }
```
- На этой машине мало памяти: при двух параллельных сборках — `-j 4` или `-j 2`.
- Если линковка падает с `LNK1104` — висит старый тестовый процесс: `Get-Process | ? Path -like '*Spark\target*' | Stop-Process -Force`.
- Доки API после изменений в `docs/spark.d.luau` или реестре: `$env:SPARK_BLESS=1; cargo test -p spark-api`.
- Golden: `target\debug\spark.exe golden examples/<x> --golden examples/_golden/<x>.png --frames 40 --size 480x270 --tolerance 0.05 --max-bad 0.15`.

## Что делать дальше (по порядку)
1. **Открыть PR** `overhaul/phase-0-1` → `main` (CI запускается только на PR/`main`): проверит cargo-deny (A9) и golden (B-8). Потом сделать golden блокирующими.
2. **Хвосты этапа C** (каждый — маленький PR):
   - ✅ 2026-11-23 сделаны: `script` в реестре (`201cf34`), сбои объектов в `check --json`, camera/tags в JSON devtools, `--help` у golden/screenshot/inspect/diff (`8881a2a`). Итог 488 тестов.
   - Осталось: запись `self.x` из скрипта обратно в сцену, `on_reload` для скриптов объектов, отметка сбойного узла в редакторе (C-16). `@environment` для команд сделан (`6d928e2`, 495 тестов).
   - Последнее состояние: 2026-11-23, недельный лимит ~98% — работа остановлена здесь.
   - Сохранять ассеты при рестарте из-за `main.luau`; скриншот живой игры через devtools (C-18, ADR 0006).
   - luau-lsp: один раз проверить, что `docs/spark.d.luau` открывается без ошибок (C-10).
3. **Этап D — редактор сцен MVP** ([11-roadmap.md](11-roadmap.md)): иерархия, инспектор из реестра (`spark_core::registry`), гизмо, undo через `spark_core::History`, сохранение через `spark_scene::save_scene`, live-правки через devtools протокол 4 (`EditorCommand::Command`). Фундамент для всего этого уже есть.
4. **Этап E — Renderer 2.0** (может идти параллельно с D): см. [04-graphics.md](04-graphics.md).
5. Старые хвосты A/B — в таблицах PROGRESS.md (⏳/⏸).

## Решения, которые ждут владельца
- Ключ minisign для подписи релизов (B-7).
- Вычистить чужие ассеты из истории git (переписывание истории).
- `docs/dev` prompts/STATE (A13).
- Открытые вопросы ADR (камера, дефолты, комментарии в `.scene`) — список в PROGRESS.md под таблицей этапа C.

## Как работали
Каждая задача — отдельному агенту (Sonnet, medium effort) с точным брифом: какие файлы, какой тест доказывает, что не трогать. Параллельные задачи — в отдельных git worktree, потом `git cherry-pick` в ветку и общий прогон. После каждого шага — строка в PROGRESS.md и запись в CHANGELOG.md (Unreleased), если видно пользователю.
