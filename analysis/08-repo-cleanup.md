# 8. Мусор и беспорядок в репозитории

Ничего из этого не удалено при разборе — только список. Каждое удаление записывать в `docs/dev/audit/CLEANUP.md`.

| # | Что | Почему мусор | Действие |
|---|---|---|---|
| J1 | `crates/spark-editor.zip` (320 КБ, коммит `c673c6e`) | Архив исходников внутри `crates/`, попал случайным коммитом; дублирует код и раздувает историю | `git rm`, добавить `*.zip` в `.gitignore` |
| J2 | `docs/dev/NEXT_PROMPT.md`, `docs/dev/EDITOR_PROMPT.md` | Промпты для ИИ-сессий на русском в публичном репозитории; содержат личные формулировки владельца | Перенести в `.claude/` или приватные заметки; в репо оставить `ROADMAP.md` |
| J3 | `docs/dev/STATE.md` | Журнал сессий («2nd session», старые заметки, путь `../Spark-baseline`) | Свести в `ROADMAP.md`/CHANGELOG, файл удалить |
| J4 | `../Spark-baseline-target` (вне репо) и упомянутый worktree `../Spark-baseline` | Временные копии для замеров | `git worktree prune`, удалить папку |
| J5 | Ветки на `origin`: `agent-b/platforms-input`, `fix/core-improvements`, `pr/1..4-*`, `spark/*` (6 шт), `wip/review-scripts` | 13 исторических веток | Владельцу: просмотреть и удалить влитые |
| J6 | `docs/API_REFERENCE.md` (24 КБ), `docs/SPARK_API.md` (55 КБ), `docs/api.json` (114 КБ), `llms.txt`, `docs/spark.d.luau` | Пять представлений одного API, частично генерируемые, частично рукописные; агент не знает, что читать | Один источник (реестр в Rust), остальное генерируется и помечено «generated, do not edit» |
| J7 | `README.md` + `README.ru.md` | Две ручные копии, расходятся (у RU на 7 КБ больше) | Тест синхронности разделов или генерация RU из EN-структуры |
| J8 | `examples/demo` и `examples/hello` (Rust) рядом с нумерованными примерами | Старые «всё в одном» примеры, CLEANUP откладывал их до замены | `demo` → `showcase/`, `hello` → `examples/rust/hello` |
| J9 | `examples/_golden/*.png` сняты на GTX 1660, а CI на lavapipe | Эталоны не соответствуют CI, шаг `continue-on-error` | Перегенерировать на CI, сделать блокирующим |
| J10 | `install.ps1`, `install.sh`, `llms.txt` в корне | Корень перегружен | `scripts/install.*`; `llms.txt` оставить (стандартное место) |
| J11 | `crates/spark-core/assets/branding/*.png` + `crates/spark-editor/assets/*.png` + `docs/assets/*.png` | Три набора логотипов | Один `assets/brand/` + `include_bytes!` |
| J12 | `branding.rs` с устаревшим doc-комментарием «shown by every windowed Spark game» | Документация врёт (сплэш opt-in) | Исправить |
| J13 | `spark-discord` в дефолтных фичах `spark-script` | Discord RPC — не дело ядра скриптинга; каждая игра тянет код IPC | Вынести в опциональный плагин, по умолчанию выкл. |
| J14 | `fbx.rs` (1247 строк) в `spark-core` | Импортёр в ядре рантайма | Отдельный крейт `spark-import` |
| J15 | Тестовый `figure.glb`, звуки `examples/demo/sounds/*.wav` без указания происхождения | Неизвестна лицензия (гл. 9) | `ASSETS.md` с источником и лицензией каждого файла |

## Предлагаемая структура корня

```
.github/  assets/  crates/  docs/  examples/  scripts/  analysis/ (этот разбор; потом в docs/dev/)
Cargo.toml  LICENSE  THIRD_PARTY_LICENSES.md (генерируется)  README.md  README.ru.md  CHANGELOG.md
CONTRIBUTING.md  SECURITY.md  CODE_OF_CONDUCT.md  ARCHITECTURE.md  llms.txt
```
