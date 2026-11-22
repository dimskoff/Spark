# Этап C: план реализации (по ADR 0003, 0004, 0005)

Решения: сцены и префабы в **TOML** (`docs/dev/adr/0003-*.md`), свой крейт **`spark-reflect`** с derive (`0004`), `Object` = узел + типизированные компоненты, команды в `spark-core`, скрипты на объектах (`0005`).

Правила: один шаг = один PR; после каждого шага зелёные `cargo test --workspace`, `spark check` на примерах, golden-скриншоты; Luau API не меняется (только добавляется). Каждый PR: запись в CHANGELOG, обновление доков. Ничего из этого пока не реализовано.

Условные обозначения: файлы указаны от корня репозитория, `crates/` опущено.

## Фаза 1. Реестр типов (код ядра не меняется по поведению)

- [ ] **C-1. Крейт `spark-reflect` (без derive).**
  Что: `Value`, `FieldType`, `FieldInfo`, `TypeInfo`, `EnumInfo`, трейт `Reflect`, `ReflectError` (с did-you-mean), валидация (тип, диапазон, конечность float). Типы реализуются вручную в тестах. Без зависимостей.
  Файлы: новый `spark-reflect/` (`Cargo.toml`, `src/lib.rs`, `value.rs`, `info.rs`, `error.rs`), корневой `Cargo.toml` (workspace member и `[workspace.dependencies]`).
  Тест: юнит-тесты `spark-reflect/tests/hand_impl.rs`: get/set по пути `"shape.radius"`, отказ NaN, выход за диапазон, неизвестное поле даёт подсказку, `from_value(to_value(x)) == x`.

- [ ] **C-2. Крейт `spark-reflect-derive`.**
  Что: `#[derive(Reflect)]`, `#[derive(ReflectEnum)]`; атрибуты `range`, `step`, `unit`, `default`, `transient`, `readonly`, `hidden`, `lua`, `clamp`; документация из `///`.
  Файлы: новый `spark-reflect-derive/`, реэкспорт из `spark-reflect`.
  Тест: те же тестовые структуры, что в C-1, но через derive, результаты идентичны ручным реализациям; тесты на ошибки компиляции (`trybuild` как dev-зависимость) для неверных атрибутов. Замер `cargo build --timings` записать в ADR 0004 (раздел Consequences).

- [ ] **C-3. Derive на данных движка.**
  Что: `#[derive(Reflect)]` для `Material`, `Body`, `Shape`, `BodyKind`, `Light`, `LightKind`, `Camera`, `Sun`, `Fog`, `Sky`, `BlendMode`. Поведение не меняется.
  Файлы: `spark-core/Cargo.toml` (+ `spark-reflect`), `spark-core/src/{material,physics,scene,camera}.rs`.
  Тест: `spark-core/tests/reflect_types.rs`: для каждого типа `from_value(to_value(default)) == default`; у каждого зарегистрированного поля непустой `doc` (тест-линтер); диапазоны совпадают с ограничениями в `with_roughness/with_metallic`; все старые тесты ядра зелёные.

- [ ] **C-4. `Object.transform` (механическая замена).**
  Что: `Object.position/rotation/scale` переезжают в `Transform`; `object.position` -> `object.transform.position`. Около 210 упоминаний, правит компилятор, логики нет. `Camera` свои поля сохраняет.
  Файлы: `spark-core/src/scene.rs` и все использования в `spark-core`, `spark-render`, `spark-physics`, `spark-script`, `spark`, `spark-editor`.
  Тест: `cargo test --workspace` без изменений в тестах (кроме правки путей в Rust-тестах); golden-скриншоты идентичны; примеры запускаются (`spark check`).

- [ ] **C-5. Реестр компонентов над `Object`.**
  Что: `ComponentInfo { get, set, add, remove }`, `registry()`, `Object::get_prop(path)` / `set_prop(path, Value)`; компоненты `transform`, `mesh`, `material`, `body`, `light`.
  Файлы: новый `spark-core/src/registry.rs`, `spark-core/src/scene.rs`.
  Тест: `spark-core/tests/registry.rs`: для всех полей реестра set затем get возвращает то же; `add/remove` для `body` и `light`; тест с исчерпывающей деструктуризацией `let Object { .. }` падает при добавлении поля без решения по реестру.

## Фаза 2. Luau-биндинги и документы из реестра

- [ ] **C-6. did-you-mean по полям из реестра.**
  Что: подсказки для неизвестного поля берут имена полей из реестра; из `OBJ_MEMBERS` удаляются имена полей (остаются методы).
  Файлы: `spark-script/src/types.rs` (`OBJ_MEMBERS`), `spark-script/src/names.rs`.
  Тест: `spark-script/tests/strict.rs` без изменений зелёный; новый тест: каждое `lua`-имя реестра есть в виртуальной машине и в подсказках.

- [ ] **C-7. Сгенерированные биндинги: transform, name, visible.**
  Что: при установке `Obj` по реестру регистрируются `add_field_method_get/set` для полей с `lua = ...`; ручные замыкания для `position/x/y/z/rotation/scale/visible/name` удаляются.
  Файлы: `spark-script/src/types.rs`, новый `spark-script/src/reflect_bind.rs`, `spark-script/src/convert.rs`.
  Тест: весь `spark-script/tests/` без изменений; микробенчмарк (чтение и запись `obj.position` на 10k объектах, в тесте с порогом) не хуже старого более чем на 10 %, числа записать в PR.

- [ ] **C-8. Сгенерированные биндинги: material.**
  Что: то же для `color, blend, double_sided, cast_shadow, pbr, metallic, roughness, emissive, data`; адаптеры `material`, `shader`, `normal_map`, `orm_map` остаются вручную.
  Файлы: `spark-script/src/types.rs`, `reflect_bind.rs`.
  Тест: `tests/scripts/*` и golden-примеры без изменений; текст ошибок для неверного значения (`obj.metallic = "x"`) совпадает со старым (снимок).

- [ ] **C-9. Сгенерированные биндинги: body, light; удаление `PROPS`.**
  Что: скалярные поля `velocity, angular_velocity, mass, gravity_scale, sensor` и поля света; `apply_props` (spawn-таблица) валидируется через реестр; строка `PROPS` и мёртвые замыкания удаляются.
  Файлы: `spark-script/src/{types,physics,light}.rs`.
  Тест: `tests/scripts/*`, `examples/03-physics` и `examples/showcase` дают те же golden; `spawn{ colour = ... }` даёт подсказку `color`.

- [ ] **C-10. `spark.d.luau` и `api.json` из реестра.**
  Что: блок `Object`, `SpawnProps`, `BodySpec`, `LightSpec` генерируется между маркерами `-- @generated components begin/end`; в `api.json` секция `components`.
  Файлы: `spark-reflect/src/luau_decl.rs`, `spark-api/src/{lib,render}.rs`, `spark-api/tests/generated.rs`, `docs/spark.d.luau`, `docs/api.json`, `docs/API_REFERENCE.md`.
  Тест: `SPARK_BLESS=1 cargo test -p spark-api` затем обычный прогон: сгенерированное актуально; `spark-script/tests/api_drift.rs` зелёный; luau-lsp открывает `spark.d.luau` без ошибок (проверить вручную один раз).

## Фаза 3. Команды

- [ ] **C-11. Рецепты мешей.**
  Что: `Assets` запоминает `MeshSource` (примитив с параметрами, ассет + узел, либо runtime-only) для каждого `MeshId`; `Mesh.cube/box/sphere/...` и загрузка glTF его заполняют.
  Файлы: `spark-core/src/assets.rs`, `spark-core/src/mesh.rs`, `spark-core/src/model.rs`, `spark-script/src/api.rs`.
  Тест: юнит: `Mesh.box(1,2,3)` даёт recipe `box [1,2,3]`, `Mesh.custom` даёт runtime-only; повторное создание меша из recipe даёт те же вершины.

- [ ] **C-12. `NodeId`, `Command`, `World::apply`, `History`.**
  Что: ленивые стабильные id у объектов, `HashMap<NodeId, ObjectId>` в `World`; `Command` (`Spawn`, `Delete`, `SetProperty`, `Reparent`, `AddComponent`, `RemoveComponent`, `Batch`), обратные команды, `History` со слиянием перетаскивания. `Spawn` пока принимает минимальный `SceneDoc`.
  Файлы: новые `spark-core/src/{command,node_id,history}.rs`, `spark-core/src/{scene,world}.rs`.
  Тест: `spark-core/tests/commands.rs`: для каждой команды apply затем inverse возвращает `dump_json` бит в бит; случайная последовательность из 200 команд с фиксированным seed, затем undo всего равно исходной сцене, затем redo всего равно конечной; `Batch` откатывается целиком при ошибке; цикл в `Reparent` отвергается.

- [ ] **C-13. Devtools протокол 4.**
  Что: `EditorCommand::Command { json }`; `SetObject` остаётся как совместимая надстройка над `Batch`; `ObjectInfo` получает `node_id`; игра применяет через `World::apply`. Редактор пока не меняется (его инспектор делает Этап D).
  Файлы: `spark-devtools/src/protocol.rs`, новый `spark-devtools/src/command_json.rs`, `spark/src/devtools.rs`.
  Тест: протокольные тесты `spark-devtools` (round-trip JSON, старый `SetObject` даёт те же изменения), интеграционный тест игры: `SetProperty "body.mass"` меняет массу, значение вне диапазона отклоняется с кодом ошибки.

## Фаза 4. Сцены и префабы

- [ ] **C-14. `SceneDoc` и крейт `spark-scene`: формат `.scene` v1.**
  Что: `capture(world)` / `instantiate(doc, world)` в ядре; `spark-scene`: TOML <-> `SceneDoc`, канонический писатель (порядок полей, пропуск значений по умолчанию, градусы, hex-цвета), ошибки с номером строки, каркас миграций (`format_version`), `spark migrate --check`.
  Файлы: новые `spark-scene/`, `spark-core/src/doc.rs`, `spark-cli/src/` (команда `migrate`), `docs/` (описание формата).
  Тест: `spark-scene/tests/roundtrip.rs`: golden `.scene` из ADR 0003, load -> save даёт тот же файл побайтно; дубликат и висячий id, неизвестное поле (с подсказкой), неверный тип дают ошибки с номером строки; тестовая миграция v0 -> v1 на фикстуре; файл версии выше известной отвергается понятным сообщением.

- [ ] **C-15. Префабы, overrides, Luau `scene.load/save`, `spawn(prefab)`.**
  Что: `.prefab`, экземпляр с `prefab = "..."`, `[node.overrides]`; при сохранении overrides вычисляются сравнением с свежим экземпляром префаба; Luau `scene.load(path, {replace=})`, `scene.save(path)`, `spawn("x.prefab", props)`; запись в `spark.d.luau` и доки.
  Файлы: `spark-core/src/doc.rs`, `spark-scene/src/prefab.rs`, новый `spark-script/src/scene_api.rs`, `docs/spark.d.luau`, пример `examples/08-scenes/` (+ golden).
  Тест: сценарный тест Luau: загрузить уровень, найти объект, `spawn` префаб с `position`, сохранить и перечитать; тест overrides: изменённый цвет попадает в файл, сброшенный к значению префаба исчезает; `api_drift.rs` зелёный.

## Фаза 5. Скрипты на объектах и остальное

- [ ] **C-16. Компонент `script`, `props`, изоляция ошибок.**
  Что: `script = "src/door.luau"` + `props`; экземпляр с `self.object` и проксированием членов `Object`; обратные вызовы `start/update/fixed_update/late_update/on_collision/on_destroy`; порядок по монотонному номеру создания; сбой одного объекта переводит его в `Failed`, игра идёт дальше; сообщение с `NodeId`.
  Файлы: новые `spark-script/src/object_scripts.rs`, `spark-script/src/{lib,runtime}.rs`, `spark-core/src/scene.rs` (поле `script`), `spark-devtools/src/protocol.rs` (`ScriptError` с `node_id`).
  Тест: `spark-script/tests/object_scripts.rs`: два объекта, ошибка в одном, второй продолжает `update`, глобальная игра не остановлена; неизвестное имя в `props` из файла даёт ошибку с подсказкой; коллизия имени prop с членом `Object` отвергается; бесконечный цикл в одном объекте обрывается watchdog-ом и не роняет остальные.

- [ ] **C-17. `tags`, камера и окружение.**
  Что: компонент `tags` и Luau `obj.tags`, `obj:has_tag()`, `find_all_tagged()` (только добавление); `[environment]` сцены; компонент `camera` и проксирование глобала `camera` на активный узел камеры.
  Файлы: `spark-core/src/{scene,registry,camera}.rs`, `spark-script/src/types.rs`, `docs/spark.d.luau`.
  Тест: существующие тесты камеры без изменений при отсутствии узла камеры; новый тест: сцена с узлом камеры, `camera.position = ...` двигает узел, сохранение даёт ту же позицию в файле.

- [ ] **C-18. Hot reload модуля и `spark mcp` (пункты дорожной карты Этапа C).**
  Что: перезагрузка только изменённого модуля с `on_reload(old_state)` без перезапуска игры и без перезагрузки ассетов; `spark mcp`: `scene_read`, `scene_apply(commands)`, `attach` к живой игре. При необходимости отдельный короткий ADR на протокол.
  Файлы: `spark-script/src/modules.rs`, `spark-cli/src/` (mcp), `spark-devtools/`.
  Тест: замер: сохранение модуля в игре с 50 моделями < 100 мс; MCP-тест: `scene_apply` с неверной командой возвращает структурированную ошибку, с верной возвращает список обратных команд, и их применение возвращает исходную сцену.

## Порядок и зависимости

C-1 -> C-2 -> C-3 -> C-4 -> C-5 -> C-6..C-10 -> C-11 -> C-12 -> C-13; C-14 требует C-5, C-11, C-12; C-15 требует C-14; C-16 требует C-12 и C-14; C-17 после C-15; C-18 последним. Шаги C-1, C-2, C-6 безопасны почти полностью (новый код или замена списка имён). Шаг C-4 самый шумный по числу правок, но без логики. Шаги C-7..C-9 и C-17 (камера) задевают горячий путь Luau и проверяются бенчмарком и существующими тестами.

## Метрики и порог готовности Этапа C

Все пункты таблицы «Этап C» из `analysis/11-roadmap.md` закрыты: 3 ADR (готовы), derive с юнит-тестами (C-1..C-3), Luau API без изменений (C-4..C-9), генерация документов (C-10), `.scene`/`.prefab` с round-trip (C-14, C-15), команды с undo/redo (C-12), скрипты на объектах (C-16), hot reload и MCP (C-18).
