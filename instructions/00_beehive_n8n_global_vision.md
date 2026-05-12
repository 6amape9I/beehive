# Beehive + n8n: глобальное видение интеграции

## 0. Контекст

Beehive — локальный desktop/runtime-инструмент для stage-based JSON pipeline.

Главная продуктовая цель:

```text
оператор без программирования создаёт stages и pipelines →
прикладывает входные JSON-файлы в рабочее пространство →
Beehive прогоняет сущности через n8n workflow →
каждый output item сохраняется туда, куда осознанно указал workflow
```

Оператор работает с папками и этапами, а не с кодом.

n8n отвечает за бизнес-преобразование данных. Beehive отвечает за orchestration, filesystem safety, runtime state, retry, audit и сохранение артефактов.

## 1. Стратегическое решение по контракту с n8n

Теперь Beehive отправляет в n8n только полезную нагрузку:

```text
POST body = source_file.payload
```

Beehive больше не отправляет в n8n свои технические поля:

```text
entity_id
stage_id
entity_file_id
attempt
run_id
meta.beehive
```

Эти данные нужны программе, а не workflow. Beehive обязан отслеживать их внутри SQLite, `stage_runs`, `entity_stage_states`, `entity_files`, `app_events`.

Иными словами:

```text
n8n sees business JSON only
Beehive tracks runtime metadata internally
```

## 2. Входной Beehive artifact остаётся обёрнутым

Файл в stage input folder всё ещё должен быть Beehive artifact:

```json
{
  "id": "entity-001",
  "current_stage": "raw",
  "next_stage": "semantic_split",
  "status": "pending",
  "payload": {
    "raw_text": "...",
    "source_name": "..."
  },
  "meta": {}
}
```

Но HTTP request в n8n должен получить только:

```json
{
  "raw_text": "...",
  "source_name": "..."
}
```

Это позволяет не переписывать n8n workflow под внутренний runtime-формат Beehive.

## 3. Стратегическое решение по `save_path`

`save_path` — новый осознанный механизм ветвления output внутри одного n8n workflow.

Если workflow возвращает несколько item'ов, каждый item может указать свою папку назначения:

```json
[
  {
    "entity_name": "замок",
    "save_path": "main_dir/processed/raw_entities"
  },
  {
    "target_entity_name": "мобильный телефон",
    "save_path": "main_dir/processed/raw_representations"
  }
]
```

Beehive должен воспринимать `save_path` не как слепую команду записи куда угодно, а как безопасный route внутри workdir.

Минимальный MVP-принцип:

```text
save_path должен указывать на input_folder одного из активных stages
```

Если `save_path` не попадает в активный stage input folder, Beehive не пишет файл и переводит execution в blocked/error state.

## 4. Совместимость с текущими n8n workflow

В старых workflow встречается форма:

```text
/main_dir/processed/raw_entities
```

Для B0 это можно поддержать как legacy logical path, а не как абсолютный Unix path.

То есть строка:

```text
/main_dir/processed/raw_entities
```

нормализуется для сравнения как:

```text
main_dir/processed/raw_entities
```

Важно: это не разрешение писать в `/main_dir` на Linux. Это только совместимость с логическим префиксом, если stage input_folder в `pipeline.yaml` задан как `main_dir/processed/raw_entities`.

Любые другие абсолютные OS-paths должны быть запрещены.

## 5. Главная цель текущего трека

Наша ближайшая цель — не UI, не n8n REST API и не background daemon.

Главная ближайшая цель:

```text
один JSON-файл из входной raw/stage папки проходит через Beehive runtime,
отправляется в n8n payload-only,
n8n возвращает output items,
Beehive сохраняет outputs по save_path,
следующие stage states появляются и могут быть выполнены дальше.
```

Это должно работать как на Windows, так и на Ubuntu.

## 6. Что остаётся за рамками B0

На B0 не делать:

```text
визуальный graph builder
редактор n8n workflow
n8n REST API integration
credential manager
background daemon
автоматический run-until-idle service
сложные approval rules
UI polish
ручной импорт всех реальных workflow
финальную production-перегенку 22k entities
```

B0 должен убрать технические блокеры, мешающие реальному сквозному прогону.

## 7. Безопасность filesystem

Beehive никогда не должен позволять n8n писать произвольно в filesystem.

Запрещено:

```text
..
абсолютные OS paths
Windows drive paths вроде C:\...
UNC paths
пути за пределы workdir
пути в неактивные stages
перезапись чужого файла
```

Разрешено:

```text
relative save_path внутри workdir,
который совпадает с active stage input_folder
```

Legacy exception:

```text
/main_dir/... можно трактовать только как logical path main_dir/...
```

## 8. Cross-platform принцип

Код должен одинаково работать на Windows и Ubuntu.

Следствия:

```text
не использовать cmd.exe/PowerShell как обязательную часть verification;
не хардкодить backslash или slash как единственный separator;
не считать `/main_dir/...` настоящим абсолютным Linux path;
не считать Windows drive path валидным save_path;
не требовать Tauri GUI для backend tests;
не требовать n8n live server для unit/integration tests.
```

Реальные n8n URL используются только вручную или в operator docs. Automated tests должны использовать local mock HTTP server.

## 9. Главный контракт B0

В B0 Beehive должен поддержать:

```text
source Beehive artifact → scan → pending state
run due task → POST payload only to workflow_url
n8n response → one or many output objects
each output object → route by save_path if present
target artifact → Beehive-wrapped JSON in target active stage input folder
source state → done
target state → pending
stage_run → stores payload-only request and response/audit
```

Если output object не имеет `save_path`, допустим fallback на старый `next_stage`, чтобы не ломать существующую модель.

Если нет ни `save_path`, ни `next_stage`, но workflow вернул output objects, это не должно молча теряться. Такой случай должен быть blocked/contract error, кроме терминального stage, который реально возвращает пустой output.
