# Forge MCP vs Codebase Memory MCP

Дата: 2026-09-29
Повний вимір: [`mcp_tool_comparison_results.json`](mcp_tool_comparison_results.json)
Відтворюваний стенд: [`mcp_tool_comparison_benchmark.py`](mcp_tool_comparison_benchmark.py)

## Висновок

- Для локальних coding-agent задач на цьому репозиторії Forge краще як основний інструмент: нижча затримка більшості symbol/source/impact/AST/documentation запитів, компактніші відповіді та спеціалізовані пояснення freshness, unresolved references і removal safety.
- Codebase Memory MCP доповнює Forge ширшим графом, архітектурними зрізами, data-flow/cross-service tracing і runtime trace ingestion. Його `query_graph` дає гнучкість, але вимагає від агента написати й інтерпретувати Cypher.
- У цьому прогоні Forge SQLite займає 8.52 MB; ізольований Codebase cache — 13.14 MB. Codebase індексує більше зв'язків, але індекси мають різний file scope та типи ребер, тому це не є нормалізованим тестом «якість на байт».
- У Forge є явний реєстр unresolved/ambiguous resolver evidence: 11,221 unresolved+ambiguous записів, з них 11,207 `unresolved` і 14 `ambiguous`; 3,524 resolved, 320 external, 0 parse errors. Codebase повідомляє 0 skipped, 0 parse-partial і 0 not-indexed files, але не має сумісного загального лічильника unresolved references. Його `CALL_REFERENCE` edges не можна трактувати як цей лічильник.
- Codebase `index_status` і file coverage сильні для перевірки стану індексу, але `no_recorded_issue` із `signal=best_effort` прямо не доводить повноту графа.
- Для видалення символу різниця суттєва: Forge відповів `blocked`, показав один inbound `CALLS` і 93 unresolved refs target; Codebase повернув графові рядки без safety verdict і без unresolved count.

## Методика

- Бінарники: `contextunity-forge-mcp` 0.2.0 і `codebase-memory-mcp` 0.10.8; обидва сервери викликані напряму через MCP stdio. Codebase MCP у глобальній Kilo-конфігурації вимкнений; конфігурацію не змінювали.
- Для кожного сценарію обидва сервери читають один immutable temporary source snapshot із однаковими відносними шляхами та цільовими символами. Forge підкоряється `forge-mcp.yaml` roots; Codebase запускався в `mode=full` над усіма скопійованими root files, включно з manifest/config файлами. AST і checkpoint сценарії окремо rerun-нуті з тією ж copy policy та п'ятьма повторами; їхній source-copy metadata збережений у `scenario_overrides`. Тому кількість вузлів/ребер, огляд workspace і disk/cache bytes не є порівнянням однакового indexed corpus.
- З тестового snapshot вилучені `.git`, `.forge`, `.codebase-memory`, `target`, `vendor`, `AGENTS.md`, `.agents`, `skills` і benchmark-артефакти. Codebase cache, HOME, XDG та daemon були ізольовані в тимчасовій теці. У timed runs використовувались лише тимчасовий Forge DB та Codebase cache; benchmark не запускав mutating Codebase tools і не змінював source files/global config.
- Перевірено 17 Forge-сценаріїв для 15 унікальних tools (три `code_map_analyze` cases), 16 найближчих Codebase відповідників і 2 Codebase-only low-confidence diagnostics. На кожен scenario/backend: 5 cold-process і 5 warm-process вимірів. У JSON — 70 рядків, по 5 вимірів у кожному: 170 Forge + 180 Codebase tool calls.
- `cold_process`: новий Forge server/SQLite connection або перезапуск ізольованого Codebase daemon/graph connection для кожного повтору. Індекс свіжопобудований один раз і зберігається між повторами; Linux page cache не очищався. `warm_process`: один discarded warm-up, далі п'ять запитів у тому самому MCP stdio client; Forge повторно використовує server/SQLite connection, Codebase — окремий daemon/graph connection за stdio proxy. Між сценаріями Codebase daemon перезапускається; у реальній довгоживучій сесії цей startup amortized між усіма tools.
- `wall_ms` охоплює `tools/call` після MCP initialize; cold process startup наведений окремо. Це не end-to-end час першого запуску агента. У Codebase новий daemon запускався для cold-вимірів, тож startup зазвичай займає секунди; у звичайній довгоживучій сесії цей cost амортизується.
- CPU — дельта Linux `schedstat` по потоках відстежуваних процесів/daemon; RSS — сума одночасного `VmRSS` із семплом кожні 5 ms. `VmHWM` у JSON є сумою process-lifetime highs, не одночасним піком. Короткоживучі дочірні процеси між семплами та діти, створені non-leader threads, можуть не потрапити у вимір. Пошук Codebase daemon через `/proc` виконується sampler-ом і його невеликий overhead залишається в `wall_ms`.
- Локальний runner не має RPC timeout, не дренує stderr під час роботи сервера і не відповідає на server-initiated JSON-RPC requests; усі відомі локальні виклики завершилися. Не використовуйте цей одноразовий harness проти нестабільного/віддаленого сервера без hardening.
- У таблицях нижче час і ресурси — median п'яти повторів. JSON зберігає кожен повтор, content/wire bytes, CPU, RSS, process count і response hash. Хеш включає JSON-RPC request ID, тому відмінність warm-хешів сама по собі не означає відмінний tool result.
- `content bytes` — текст MCP content; `wire bytes` — stdout bytes до відповідної відповіді. Codebase може передавати одночасно текст і `structuredContent`; це не тотожно кількості токенів, оцінка токенів `bytes/4` лише приблизна.
- Forge tool schemas звірені з `src/mcp/tools.rs` і [`docs/reference/mcp-tools.md`](../docs/reference/mcp-tools.md); Codebase schemas отримані через live `tools/list` від встановленого 0.10.8 binary. Upstream tool registry: [`DeusData/codebase-memory-mcp` v0.10.8](https://github.com/DeusData/codebase-memory-mcp/tree/v0.10.8).
- Якість та agent clarity оцінені вручну за evidence прикладами з JSON, не LLM-judge. Q (якість): 5 — прямий, scoped, достатній результат; 3 — корисний, але приблизний/потребує додаткової інтерпретації; 1 — немає сумісної функції. Clarity (зрозумілість): структура, явність доказів/обмежень, кількість ручних кроків.
- `session_checkpoint` benchmark обмежений read-only `action=list` у тимчасовому snapshot без `.forge/checkpoints.json`; п'ять повторів підтвердили порожню відповідь `{}`. Response examples вилучені з artifact; залишені байтові метрики та hashes.
- AST query спершу дав 0 збігів через некоректний шаблон без return type. Цей результат відкинутий; фінальна таблиця використовує позитивний шаблон, який обидва інструменти перевірили на `Server::admit`.

## Контракти інструментів

Forge спільні page fields: `limit` (default 30, max 100), `offset`, `detail=compact|full`, `generation`. Уточнені core inputs і найближчий Codebase контракт:

| Forge tool | Основні входи Forge | Найближчий Codebase контракт |
|---|---|---|
| `code_map_overview` | page fields | `get_architecture(project, path?, aspects?)`, `index_status(project)`, `get_graph_schema(project)` |
| `code_map_search` | `pattern` required; `kind?`, `path?`, `group_by_file?`, `include_docs?`, page | `search_graph(project, query/name_pattern/qn_pattern/file_pattern/label/relationship/fields/limit/offset)` |
| `code_map_inspect` | `selector` required; `show_doc`, `show_source`, `leading_lines`, `max_body_lines`, `source_offset`, page | Комбінація `search_graph` і `get_code_snippet(project, qualified_name)` |
| `get_code_snippet` | `selector` required; source window/offset і page | `get_code_snippet(project, qualified_name, include_neighbors?)` |
| `code_map_explain` | `selector` required; `direction?`, docs/source controls, page | `query_graph(project, query)` або `trace_path(project, function_name, direction, depth)` |
| `code_map_impact` | `selector` required; `depth`, page | `trace_path(project, function_name, direction, depth, mode, include_tests, edge_types)` |
| `code_map_tests` | `selector` required; `direction=inbound|outbound`, page | `query_graph` по `TESTS`; `trace_path(include_tests?)` |
| `code_map_prove_removal` | `selector` required, page | Немає removal-proof API; лише `query_graph` / `search_graph` наближення |
| `code_map_query` | `operation` required; `selector?`, `depth`, page | `query_graph(project, query, graph=code|missed, max_rows)`; direct analog лише для Cypher |
| `code_map_analyze` | `target` required; `include_cycles?`, `lint?`, page; bounded read-only SQL | `index_status(project)`, `check_index_coverage(project, paths|scopes)`, `get_architecture(aspects=['cycles'])` |
| `ast_grep_search` | `pattern` і `language` required; `path?`, page | `search_code(project, pattern, regex?, path_filter?, mode)` — текст, не AST |
| `search_docs` | `query` required; `doc_type?`, `component?`, `include_excerpt?`, page | Немає doc-search API; workaround `search_code` |
| `get_doc` | `path_or_id` required; `section?`, page | Немає generic Markdown read API; `search_code` дає match-centered source windows |
| `session_checkpoint` | `action` required; `name?`, `content?` | Немає checkpoint API; `manage_adr` — окремий ADR контракт |
| `forge_guide` | `topic?`, `force?` | Немає MCP guide; `get_graph_schema(project)` повертає лише схему графа |

## Індекс та старт

| Метрика | Forge | Codebase Memory MCP |
|---|---:|---:|
| Індексний розмір на диску | 8,519,680 B / 8.52 MB / 8.12 MiB | 13,139,122 B / 13.14 MB / 12.53 MiB cache |
| Nodes | 1,893 | 1,986 |
| Edges | 3,971 | 9,109 |
| Files, як їх рахує інструмент | 109 | 112 `File` nodes у `get_graph_schema` |
| Doc sections | 49 | 49 `Section` nodes |
| Index build wall | 1,681 ms (повна команда build) | 6,352 ms (`index_repository` tool call) |
| Server/daemon startup | 2.8 ms median cold | 5.4 s median cold; окремий index setup startup у цьому прогоні 8.24 s |
| Index build CPU | 2,784 ms | 7,334 ms для tool call |
| Index build sampled peak RSS | 64.6 MiB | 215.0 MiB |
| Codebase coverage | — | `not_indexed=0`, `skipped=0`, `parse_partial=0`, `artifact_present=false` |

Codebase first isolated index setup wall становить близько 14.59 s, якщо скласти 8.24 s daemon startup і 6.35 s `index_repository` call; це не включає конфігураційні setup commands. Forge 1.68 s — повний виміряний build command.

У `get_graph_schema` Codebase найбільші edge types: `USAGE` 2,888, `CALLS` 2,508, `DEFINES` 1,838, `DEFINES_METHOD` 505; також є `SIMILAR_TO` 229, `SEMANTICALLY_RELATED` 223 та інші типізовані зв'язки. Це дає багатший граф, але загальна кількість edges не є прямим виміром правильності: edge taxonomy та indexed file scope відрізняються.

Codebase cache у 1.54× більший за Forge SQLite у цьому конкретному запуску; Forge на ~35% компактніший. Оскільки Codebase рахує cache directory, а не один еквівалентний SQLite artifact, і `mode=full` охоплює ширший набір файлів/ребер, цей відсоток — лише практичний footprint benchmark-конфігурацій, не чистий форматний overhead.

## Затримка та розмір відповіді

Час наведено як `cold/warm ms`; розмір як `warm text/wire bytes`. Для Codebase cold startup до цих значень не входить.

| Сценарій (Forge / найближчий Codebase tool) | Forge cold/warm ms | Codebase cold/warm ms | Text bytes Forge/Codebase | Wire bytes Forge/Codebase |
|---|---:|---:|---:|---:|
| Workspace overview (`code_map_overview` / `get_architecture`) | 42.49 / 28.66 | 15.22 / 11.61 | 1,333 / 1,531 | 1,605 / 1,690 |
| Symbol search (`code_map_search` / `search_graph`) | 9.83 / 2.70 | 8.34 / 11.69 | 346 / 324 | 480 / 801 |
| Symbol inspect (`code_map_inspect` / `search_graph`) | 8.78 / 2.97 | 11.75 / 5.93 | 3,057 / 324 | 3,549 / 801 |
| Source (`get_code_snippet` / `get_code_snippet`) | 10.40 / 4.42 | 11.54 / 6.37 | 2,403 / 6,266 | 2,631 / 12,860 |
| Tests (`code_map_tests` / `query_graph`) | 10.29 / 3.47 | 166.17 / 33.04 | 1,847 / 482 | 2,137 / 576 |
| Unresolved (`code_map_analyze` / `query_graph`) | 29.53 / 21.61 | 143.95 / 56.90 | 1,723 / 712 | 2,011 / 807 |
| Impact (`code_map_impact` / `trace_path`) | 13.41 / 4.05 | 88.45 / 11.53 | 1,026 / 313 | 1,272 / 767 |
| Symbol explain (`code_map_explain` / `query_graph`) | 14.56 / 8.08 | 144.23 / 19.84 | 4,557 / 5,609 | 5,233 / 5,731 |
| Graph slice (`code_map_query(slice)` / `query_graph`) | 6.45 / 1.71 | 152.56 / 22.32 | 1,682 / 5,609 | 2,012 / 5,731 |
| Workspace diagnostics (`code_map_analyze` / `index_status`) | 38.94 / 26.00 | 9.31 / 8.78 | 2,377 / 382 | 2,731 / 919 |
| Path diagnostics (`code_map_analyze` / `check_index_coverage`) | 9.62 / 5.96 | 10.08 / 9.07 | 4,725 / 787 | 5,434 / 1,755 |
| Removal safety (`code_map_prove_removal` / `query_graph`) | 28.73 / 20.85 | 352.44 / 214.44 | 1,353 / 435 | 1,583 / 531 |
| AST (`ast_grep_search` / `search_code`) | 10.70 / 5.72 | 14.10 / 16.44 | 12,390 / 3,172 | 12,922 / 6,603 |
| Documentation search (`search_docs` / `search_code`) | 7.29 / 2.45 | 19.77 / 21.56 | 901 / 1,739 | 1,086 / 3,666 |
| Documentation read (`get_doc` / `search_code`) | 6.71 / 1.83 | 17.39 / 14.74 | 12,761 / 3,561 | 13,446 / 7,438 |
| Tool guide (`forge_guide` / `get_graph_schema`) | 1.11 / 0.35 | 30.48 / 30.37 | 1,424 / 5,056 | 1,546 / 11,019 |
| Checkpoint list (`session_checkpoint`) | 0.84 / 0.33 | n/a | 2 / n/a | 92 / n/a |

Виклики з однаковими іменами семантично ближчі; інші рядки є найкращими наближеннями, а не інтерфейсною паритетністю. Наприклад, Codebase `query_graph` у table є Cypher, а Forge `code_map_query(slice)` — готова bounded operation; відношення latency не слід читати як порівняння однакової роботи.

## CPU та RSS

Нижче — warm median; значення cold і всі індивідуальні виміри є в JSON.

| Сценарій | CPU ms Forge/Codebase | Sampled peak RSS MiB Forge/Codebase |
|---|---:|---:|
| Workspace overview | 25.40 / 0.83 | 14.0 / 13.4 |
| Symbol search | 2.64 / 0.80 | 12.9 / 13.4 |
| Symbol inspect | 2.73 / 0.70 | 12.9 / 13.4 |
| Source snippet | 4.31 / 1.09 | 13.7 / 13.4 |
| Tests | 3.36 / 12.08 | 14.3 / 23.0 |
| Unresolved evidence | 21.45 / 25.82 | 14.6 / 23.1 |
| Impact | 3.79 / 2.22 | 12.6 / 13.4 |
| Symbol explain | 5.65 / 4.50 | 12.8 / 13.8 |
| Graph slice | 1.67 / 1.31 | 12.7 / 13.6 |
| Workspace diagnostics | 25.66 / 0.54 | 14.9 / 13.4 |
| Path coverage | 5.68 / 1.49 | 12.6 / 13.4 |
| Removal safety | 20.65 / 194.80 | 14.2 / 61.5 |
| AST search | 5.56 / 0.84 | 15.0 / 13.4 |
| Documentation search | 2.36 / 1.35 | 12.6 / 13.4 |
| Documentation read | 1.68 / 0.99 | 12.0 / 13.4 |
| Tool guide/schema | 0.33 / 16.90 | 7.7 / 16.4 |
| Checkpoint list | 0.31 / n/a | 7.8 / n/a |

Пер-процес CPU в Codebase може бути значно меншим за wall time, коли запит очікує на graph daemon/IPC; важливо разом дивитися на обидві метрики. `query_graph` removal probe — явний hot spot: близько 195 ms CPU, 214 ms wall та 61.5 MiB sampled RSS на warm request проти Forge `code_map_prove_removal` близько 21 ms CPU, 21 ms wall і 14.2 MiB. Це не тотожні tool contracts: Codebase повертає ребра для ручної інтерпретації, Forge формує risk verdict.

## Інструмент за інструментом

Оцінки `Q/clarity` — `Forge/Codebase`, шкала 1–5 з методики. `n/a` означає відсутність відповідника.

| Forge tool | Codebase MCP відповідник | Еквівалентність | Q F/C | Clarity F/C | Спостереження для агента |
|---|---|---|---|---|---|
| `code_map_overview` | `get_architecture` (+ `index_status`, `get_graph_schema`) | близькі огляди, різні акценти | 5/4 | 5/4 | Forge повертає components/coverage/freshness; Codebase має architecture aspects і rich graph schema. Codebase швидший в цьому overview call. |
| `code_map_search` | `search_graph` | близький symbol lookup | 5/5 | 5/4 | Обидва знаходять заданий symbol/file; Forge дає компактну symbol-oriented форму, Codebase — qn/label/edges та BM25/regex/semantic filters. |
| `code_map_inspect` | `search_graph` + `get_code_snippet` | немає одного відповідника | 5/3 | 5/3 | Forge з'єднує selector resolution, docs, metadata та references; `search_graph` у Codebase — лише пошуковий рядок, source треба окремо прочитати. |
| `get_code_snippet` | `get_code_snippet` | прямий функціональний аналог | 4/5 | 5/4 | Обидва знаходять `Server::admit`. Forge віддає bounded preview (~2.4 KB), Codebase — весь метод (~6.3 KB), що повніше, але більш token-expensive. |
| `code_map_tests` | `query_graph` по `TESTS` | наближення через граф | 4/3 | 5/3 | Для `Server::read` Forge показав 4 scoped/transitive test entries та 19 unresolved refs; Codebase повернув 2 прямі `TESTS` edges. Forge ширший, але попереджає про lexical fallback; Codebase простіший, але має менше witnesses. |
| `code_map_analyze` unresolved | `query_graph` по `CALL_REFERENCE` | нееквівалентні показники | 5/2 | 5/2 | Forge агрегує statuses/causes/top files. Codebase повертає окремі call-reference graph rows; додатковий custom low-confidence query знаходить candidates, але не є canonical unresolved count. |
| `code_map_impact` | `trace_path` | близьке incoming traversal | 4/4 | 5/4 | Обидва дають dependency paths. `trace_path` має calls/data-flow/cross-service режими; Forge додає paging і relation evidence. Порівнювати потрібно witnesses, не лише ms/counts. |
| `code_map_explain` | `query_graph` / `trace_path` | наближення | 5/3 | 5/2 | Forge повертає node ownership/docs/incoming/outgoing у пояснювальному контракті; Codebase повертає Cypher таблицю з `type(r)` та вимагає synthesis від агента. |
| `code_map_query` | `query_graph` | прямий лише для Cypher | 4/5 | 5/3 | Codebase Cypher гнучкий; Forge додає bounded `slice`, `unwired` і обмежений query model. Складні граф-запити в Codebase помітно дорожчі в цьому тесті. |
| `code_map_analyze` workspace | `index_status` / `get_architecture(aspects=['cycles'])` | частковий | 5/3 | 5/4 | Forge бачить unresolved/status/cycles. Codebase status дуже зрозумілий для skipped/partial/not-indexed inventory, але не замінює graph diagnostics. |
| `code_map_analyze` file | `check_index_coverage` | partial | 4/4 | 5/5 | Codebase вказує `metadata_match`, `no_recorded_issue` і власний best-effort caveat; Forge повертає per-file diagnostics/resolution evidence. |
| `code_map_prove_removal` | `query_graph` edge listing | нееквівалентні | 5/2 | 5/1 | Forge відповідає `blocked`, показує inbound count=1, target unresolved=93, parse_errors=0. Codebase знаходить raw edges, але safety-висновку не має. |
| `ast_grep_search` | `search_code` | нееквівалентні: AST vs text | 5/3 | 5/2 | Виправлений позитивний запит дав Forge AST match на lines 158–290. Codebase literal search знайшов той самий method/source, але не може відповісти на структурний AST-pattern запит загалом. |
| `search_docs` | `search_code` literal | workaround | 5/3 | 5/3 | Forge знайшов дві README sections з `doc_type`, rank, excerpt і anchor. Codebase знайшов README module та джерельне вікно, але без doc-type/section ranking. |
| `get_doc` | `search_code` | нееквівалентні: section read vs snippet | 5/2 | 5/3 | Forge повернув 8 README sections (~12.8 KB) з headings. Codebase повернув знайдений section/window і `source_truncated=true`, а не весь документ. |
| `forge_guide` | `get_graph_schema` | немає функціонального аналога | 5/2 | 5/2 | Forge пояснює вибір операцій, paging та budget recovery. Codebase schema корисна для Cypher, але описує labels/edges, не workflow. |
| `session_checkpoint` | — | немає аналога | 5/n/a | 5/n/a | Forge має list/get/save/delete checkpoint actions. Порівняно лише ізольований read-only list; mutating actions не викликались. |

## Unresolved та completeness

Forge `code_map_analyze(target='')` на benchmark snapshot звітує:

- `total_unresolved=11,221`, з них `unresolved=11,207` і `ambiguous=14`; `resolved=3,524`, `external=320`, `total_errors=0`.
- Причини unresolved: `dynamic_callee=3,745` (86 файлів), `no_lexical_candidate=3,617` (89), `shadowed_binding=3,453` (86), `missing_indexed_import=360` (72), `alias_target_missing=32` (28).
- Найбільші файли: `src/db/writer.rs` 815, `src/engine/linker.rs` 526, `tests/query_context.rs` 451, `tests/tool_evolution.rs` 435, `tests/mcp_freshness.rs` 400.
- Workspace parse errors — 0; cycle analysis показує 7 cycles / 11 nodes і ще 2 omitted cycles.
- Target `src/mcp/server.rs:admit` має 93 target unresolved references; explicit incoming edge — `Server::read -> Server::admit` at line 300. Тому Forge removal verdict `blocked` обґрунтований, хоча статичний proof сам попереджає про dynamic/external callers.
- Target `Server::read` test mapping має чотири candidates, `discovery_method=lexical_fallback` і 19 unresolved refs у scoped neighborhood; це evidence про можливі пропуски, не доказ що всі чотири є прямими runnable tests.

Codebase `index_status` підтверджує `parse_partial=0`, `skipped=0`, `not_indexed=0`; `check_index_coverage` для `src/mcp/server.rs` каже `no_recorded_issue` і `freshness=metadata_match`. Обидва — coverage/indexing signals, не resolver status. `get_graph_schema` містить лише 3 `CALL_REFERENCE` edges у цьому snapshot; query повертає ці конкретні graph facts, а не summary по всіх невирішених викликах.

Додаткові Codebase-only `query_graph` запити по `CALLS` у benchmark snapshot дали 383 edges із `confidence < 0.5`, згруповані за confidence/strategy. Це analyst-selected triage threshold, не офіційний unresolved статус. Приклади з `suffix_match`: `std::path::Path::new -> BoundedWriter.new` (`confidence=0.04`, 45 candidates), `Vec::new -> QueryBudget.new` (`0.02`, 45), `PathBuf::new -> BoundedWriter.new` (`0.04`, 45). За source names це weak/implausible resolutions; агент має перевіряти їх перед impact/removal claims. Найбільші confidence groups: `0.04` 118, `0.38` 64, `0.02` 38, `0.18` 33, `0.07` 25. Low-confidence summary call має median 103.97 ms warm / 213.44 ms cold; sample query — 74.91 / 199.58 ms.

| Codebase-only query | Cold/warm wall ms | Warm CPU ms | Warm peak RSS MiB | Warm text/wire bytes |
|---|---:|---:|---:|---:|
| Confidence/strategy counts (`confidence < 0.5`) | 213.44 / 103.97 | 97.85 | 33.4 | 609 / 806 |
| Candidate examples (`confidence < 0.5`, limit 30) | 199.58 / 74.91 | 58.93 | 34.1 | 6,814 / 7,076 |

Висновок: Codebase дає корисні candidate/confidence fields для ручного unresolved review, але не має еквівалента Forge resolver ledger і caller-scoped unresolved count; «Codebase має 0 unresolved» було б хибним.

## Паралельні напрями

- **Компактність без втрати якості:** Forge index уже 8.52 MB при 1,893 nodes / 3,971 edges; Codebase cache 13.14 MB при 1,986 / 9,109. Codebase має приблизно 2.29× edges, але ширший file scope і додаткові graph edge types. Bench не доводить, що можна видалити Forge таблиці/indexes без втрати. Наступний коректний експеримент — однаковий explicit file manifest, ablation по окремих derived tables/indexes, та replay усіх 17 task fixtures з перевіркою recall/witnesses разом із cold/warm latency й database bytes.
- **Звідки якість Codebase:** graph містить `CALLS`, `USAGE`, `TESTS`, `WRITES`, `OVERRIDE`, `IMPLEMENTS`, `SIMILAR_TO`, `SEMANTICALLY_RELATED`, coverage/complexity properties та інші структури. Це дає багаті architecture/data-flow/Cypher запити; нуль parse-partial не доводить повноту semantic links.
- **Що варто лишити у Forge:** дешево формувати agent-ready verdicts, bounded symbol/source operations, unresolved cause ledger, тестову lexical fallback із caveat, AST, документаційні section APIs, removal proof і operational tool guide.
- **Що варто брати з Codebase:** cross-service/data-flow/risk-labelled `trace_path`, архітектурні aspects, багатші semantic/usage edges, runtime trace ingestion і project index coverage.
- **Стабільність вимірів:** warm Codebase query latencies вищі за Forge для більшості порівнюваних narrow calls; однак cold sample показує сотні мілісекунд на graph-path/query задачах і декілька секунд startup. Для реального постійного daemon окремо оптимізувати cold startup, не змішуючи його з warmed tool latency.

## Інвентар Codebase-only

Codebase MCP рекламує 15 tools. Поза парним query matrix залишаються `list_projects`, `delete_project`, `detect_changes`, `manage_adr`, `ingest_traces`; `index_repository` запускався один раз як індексна підготовка, а не п'ять разів як query tool. `delete_project`, `manage_adr(update)` та `ingest_traces` змінюють стан і навмисно не запускались. `detect_changes` і project-management tools не мають прямого Forge tool зі співставним контрактом.

Forge теж має 15 tools; всі перевірені: `code_map_overview`, `code_map_search`, `code_map_inspect`, `get_code_snippet`, `code_map_explain`, `code_map_impact`, `code_map_tests`, `code_map_prove_removal`, `code_map_query`, `code_map_analyze`, `ast_grep_search`, `search_docs`, `get_doc`, `session_checkpoint`, `forge_guide`.

## Повторний запуск

```bash
PYTHONDONTWRITEBYTECODE=1 python tests/mcp_tool_comparison_benchmark.py --bench --repeats 5 --output tests/mcp_tool_comparison_results.json
```

Стенд створює тимчасові індекси й окремий Codebase profile під `/tmp/kilo`; користувацькі `.forge`/Codebase caches не використовуються. Результатний JSON зберігає 5 сирих повторів для кожного сценарію, sample response examples (крім checkpoint content), hashes, tool args і resource counters.
