---
title: "Milestone 022 Architectural De-bloat, Zero-Allocation Linker, and Resolution Health Overhaul"
doc_type: plan
purpose: >-
  Provide a step-by-step surgical execution manual for de-bloating Milestone 022,
  eliminating manual serialization and procedural rule duplication, wiring
  32-bit symbol/path interning across the linker, and raising self-hosted Rust
  Resolution Health from 18.5% to 65%+.
depends_on: []
---

# Milestone 022 Architectural De-bloat, Zero-Allocation Linker, and Resolution Health Overhaul

Admitted target branch: `milestone/022-declarative-framework-manifests-and-ecosystem-packs` (`.worktrees/022-declarative-framework-manifests-and-ecosystem-packs`).
Deferred workspace-split target: [Milestone PF-001](../milestones/PF-001-cold-build-rss-bound-and-linker-diet.md) (`PF-001-T05: split-cargo-workspace-into-independent-crates`).

---

## 1. Чесний аудит поточного стану гілки `022` (Чому потрібна ця перебудова)

Гілка `022` наразі додає **+28,719 рядків** (`-19,093` видалень, чистий приріст **+9,626 рядків** відносно `main`). Сумарний обсяг 22 ключових файлів індексатора та лінкера становить **36,122 рядки**:

| Файл | Поточний розмір (рядків) | Приріст у `022` vs `main` | Головна проблема (Діагноз) |
| :--- | ---: | ---: | :--- |
| [`src/engine/linker/value_flow.rs`](../../src/engine/linker/value_flow.rs) | 3,558 | +601 | Надскладний процедурний двигун потоків значень; `ScopeTypeEnv` доданий, але не підключений у гарячий шлях `lookup_tracked`, жоден рядок старих евристик не видалений; 540 рядків unit-тестів лежать у тому ж файлі. |
| [`src/engine/linker/semantic_context.rs`](../../src/engine/linker/semantic_context.rs) | 3,522 | +2,150 | Гігантські монолітні методи: `prepare_component_environments` (530 рядків), `visible` (550 рядків), `member` (342 рядки), `resolve_symbol` (380 рядків) із сотнями `String`-алокацій. |
| [`src/engine/linker/resolution.rs`](../../src/engine/linker/resolution.rs) | 2,805 | +573 | **Одна функція `resolve` на 2,645 рядків** (рядки 99–2743) із двічі скопійованим блоком ланцюжкового обходу полів `for field in fields.split('.')` (рядки 1089–1136 та 2008–2080). |
| [`src/engine/languages/dependency_registry.rs`](../../src/engine/languages/dependency_registry.rs) | 2,584 | +578 | Диспетчеризація правил побудована на рядкових `HashMap<String, Vec<usize>>` замість числових `node.kind_id(): u16` та бітової маски імпортів. |
| [`src/engine/ast/visitor.rs`](../../src/engine/ast/visitor.rs) | 2,028 | +515 | Змішані загальний обхідник `SinglePassExtractor` та специфічні для мов гілки. |
| [`src/core/semantic.rs`](../../src/core/semantic.rs) | 1,926 | +868 | Величезні DTO-структури `ValueFlowFacts` (16 векторів на кожен скоуп) та `SemanticFactRole`. |
| [`src/engine/ast/routes.rs`](../../src/engine/ast/routes.rs) | 1,606 | +1,318 | Ручний процедурний аналізатор маршрутів замість декларативних примітивів. |
| [`src/engine/languages/typescript/frameworks.rs`](../../src/engine/languages/typescript/frameworks.rs) | 1,529 | +1,378 | Окремий процедурний інтерпретатор TS/Vue/React фреймворк-правил, що дублює логіку Python-екстрактора. |
| [`src/core/typed_facts.rs`](../../src/core/typed_facts.rs) | 1,377 | +947 | Ручне перекладання 16 масивів `ValueFlowFacts` між `Node.details` JSON, `FlowStore` та `SemanticStore`. |
| [`src/db/writer/cache.rs`](../../src/db/writer/cache.rs) | 1,324 | +918 | Проміжні `FactsDto`, `NodeDto`, `FlowEntryDto`, `SemanticEntryDto`, `BinaryValue` + 700+ рядків ручного побайтового кодування кожного поля. |
| [`src/engine/linker/framework_relations.rs`](../../src/engine/linker/framework_relations.rs) | 1,297 | +1,239 | Кастомна логіка зв'язування фреймворк-відношень у лінкері. |
| [`src/engine/languages/python/render_context.rs`](../../src/engine/languages/python/render_context.rs) | 1,223 | +1,205 | Окремий процедурний рушій під Django `render()` / `get_context_data()`. |
| [`src/engine/languages/python/projections.rs`](../../src/engine/languages/python/projections.rs) | 1,153 | +1,153 | Окремий процедурний рушій проекцій Python-фреймворків. |
| [`src/db/writer.rs`](../../src/db/writer.rs) | 809 | +208 | Нагромадження `PreparedCatalogStorage` та подвійне кодування фактів (`encode_facts_ms` → JSON у `nodes.details`). |
| [`src/engine/linker/interner.rs`](../../src/engine/linker/interner.rs) | 357 | +357 | `LinkerInterner`, `SymbolId(u32)`, `PathId(u32)` написані, але **ніде не підключені** до таблиць лінкера (використовується лише `file_import_mask`). |

---

## 2. Що в архітектурі працює відмінно (НЕ ЧІПАТИ)

Наступні 4 підсистеми є фундаментом швидкодії та коректності — їхні контракти та алгоритми змінювати **заборонено**:

1. **Двофазна публікація SQLite та читацька ізоляція ([`src/db/writer.rs`](../../src/db/writer.rs), [`src/db/reader.rs`](../../src/db/reader.rs))**:
   - Побудова у тимчасовий candidate-файл із `PRAGMA journal_mode = MEMORY; synchronous = OFF; temp_store = MEMORY; cache_spill = ON;`, відкладене створення вторинних індексів (`deferred-index-batched`) після пакетної вставки, перемикання у `WAL` + `PRAGMA wal_checkpoint(TRUNCATE)` перед атомарним `fs::rename`.
2. **Гібридний 17-доменний in-memory Merkle sealing ([`src/db/writer/seal.rs`](../../src/db/writer/seal.rs), [`src/core/commitments.rs`](../../src/core/commitments.rs))**:
   - In-memory хешування 13 логічних доменів та C-API FTS5 токенізація 4 пошукових доменів у канонічному порядку `(term, docid, col, pos)`, що гарантує побітову еквівалентність `cold build == incremental delta` (`forge-owner-sha256-v2`).
3. **Крос-ворктрі бінарний кеш екстракції ([`ExtractionCache`](../../src/db/writer/cache.rs))**:
   - Контент-адресований ключ `SHA-256(source) + relative_path + grammar_id + INDEX_SEMANTICS_VERSION + effective_ruleset_hash` з атомарним `write-temp-fsync-rename` та `O_NOFOLLOW`.
4. **Однопрохідний ітеративний обхід `TreeCursor` ([`walk`](../../src/engine/ast/visitor.rs)), мемоізація `Arc<LanguageContext>` та повернення арен `libc::malloc_trim(0)`**:
   - Плоский обхід AST за один прохід, кешування `Arc<LanguageContext>` на воркснейс-скоуп у [`DependencyRegistry`](../../src/engine/languages/dependency_registry.rs) та виклики `libc::malloc_trim(0)` на фазових границях пайплайну ([`src/engine/pipeline.rs`](../../src/engine/pipeline.rs), [`src/db/writer.rs`](../../src/db/writer.rs)), які примусово повертають вільні сторінки потоків екстракції/лінкування ядру ОС і тримають RSS у межах бюджету.

---

## 3. Детальний покроковий план виконання (Блоки 1–7)

Кожен блок нижче містить: **Мета (Для чого)**, **Що ВИДАЛИТИ**, **Що ДОДАТИ / ЗАМІНИТИ**, та **Як ПЕРЕВІРИТИ**.

---

### Блок 1: Декларативний бінарний кодек (`BinaryCodec`) замість ручних DTO у `cache.rs`, `typed_facts.rs` та `writer.rs`

#### 1.1. Для чого це робити
У [`src/db/writer/cache.rs`](../../src/db/writer/cache.rs) (1,324 рядки) та [`src/db/writer.rs`](../../src/db/writer.rs) (809 рядків) написані проміжні клон-структури (`FactsDto`, `NodeDto`, `FlowEntryDto`, `SemanticEntryDto`, `UncertaintyEntryDto`, `BinaryValue`), які спочатку клонують усі рядки й вектори з `TypedFacts` у DTO (`facts_to_dto`), а потім вручну по одному полю записують їх у `Vec<u8>`. Це:
- створює зайву хвилю алокацій на кожному файлі при записі/читанні кешу;
- займає ~850 рядків однотипного бойлерплейту `write_*` / `read_*`.

#### 1.2. Що ВИДАЛИТИ
1. У [`src/db/writer/cache.rs`](../../src/db/writer/cache.rs):
   - **Видалити** проміжні структури `FactsDto`, `NodeDto`, `FlowEntryDto`, `SemanticEntryDto`, `UncertaintyEntryDto` (рядки 211–258).
   - **Видалити** функції перекладання `facts_to_dto` та `dto_to_facts` (рядки 478–562).
   - **Видалити** ручні функції `write_dto`, `read_dto`, `write_flow_facts`, `read_flow_facts`, `write_semantic_fact`, `read_semantic_fact`, `write_value_expr`, `read_value_expr`, `write_type_expr`, `read_type_expr`, `write_receiver_hint`, `read_receiver_hint` (рядки 328–477 та 679–1115 — сумарно **~580 рядків**).
2. У [`src/db/writer.rs`](../../src/db/writer.rs):
   - **Видалити** ручні функції `encode_prepared_storage`, `decode_prepared_storage`, `write_u32`, `write_str`, `write_string_slice`, `read_u32`, `read_string`, `read_string_vec` (рядки 676–868 — **192 рядки**), які дублюють примітиви з `cache.rs`.
3. У [`src/core/typed_facts.rs`](../../src/core/typed_facts.rs):
   - **Видалити** поштучне перекладання 16 полів у `FlowStore::merge_classic` (рядки 845–910) та `scope_fact_count` (рядки 430–450), замінивши їх єдиним макросом переліку полів `for_each_scope_fact_vec!`.

#### 1.3. Що ДОДАТИ / ЗАМІНИТИ
1. У [`src/db/writer/cache.rs`](../../src/db/writer/cache.rs) ввести два компактні трейти та декларативний макрос `impl_binary_codec!`:
   ```rust
   pub(crate) trait BinaryEncode {
       fn encode(&self, buf: &mut Vec<u8>);
   }
   pub(crate) trait BinaryDecode: Sized {
       fn decode(bytes: &[u8], cursor: &mut usize) -> Option<Self>;
   }
   ```
   - Реалізувати `BinaryEncode` / `BinaryDecode` для базових типів (`u8`, `u32`, `u64`, `i64`, `f64`, `bool`, ` usize`, `String`, `Option<T>`, `Vec<T>`, `BTreeMap<K, V>`, `serde_json::Value`).
   - Описати `struct_codec!` та `enum_codec!` макроси, які за 1–2 рядки генерують `BinaryEncode` / `BinaryDecode` напряму для `Node`, `Edge`, `Reference`, `DocSection`, `Diagnostic`, `ReceiverHint`, `TypeExpr`, `ValueExpr`, `ValueFlowFacts`, `SemanticFact`, `PreparedCatalogStorage` **без жодних проміжних `*Dto` структур і без клонування `TypedFacts`**.
2. У `encode_entry` / `decode_entry` кодувати `TypedFacts` напряму в `payload: Vec<u8>` зі збереженням формату `MAGIC_V3` та `SHA-256` контрольної суми.

#### 1.4. Як ПЕРЕВІРИТИ
- `wc -l src/db/writer/cache.rs src/db/writer.rs src/core/typed_facts.rs` — сумарний розмір трьох файлів має зменшитися з **3,510 рядків до ≤ 2,650 рядків (-860+ рядків)**.
- `cargo test --lib db::writer::cache` та `cargo test --lib db::writer::memory_budget_tests` — усі тести round-trip кешу та захисту від пошкоджених даних проходять.

---

### Блок 2: Плоска таблиця диспетчеризації AST за `node.kind_id(): u16` та 64-бітна Bloom-маска імпортів на вході екстракторів

#### 2.1. Для чого це робити
Під час обходу 3.3M вузлів Tree-sitter у `SinglePassExtractor` та мовних екстракторах (`projections.rs`, `typescript/frameworks.rs`) на кожному AST-вузлі виконуються:
1. Пошуки у `HashMap<String, Vec<usize>>` за рядковою назвою `node.kind()` (хешування рядка + порівняння байтів);
2. Перевірки фреймворк-правил (Django, FastAPI, Celery, DRF, React, Vue, Next.js, Express, NestJS) навіть у файлах, які взагалі не імпортують жодного з цих пакетів.

Заміна рядкового диспатчу на масив фіксованого розміру за числовим `node.kind_id(): u16` (O(1) індексація без хешування) та побітовий префільтр `(file_import_mask & rule.import_mask) == rule.import_mask` перед запуском правил відсікають 95% правил за 1 такт CPU.

#### 2.2. Що ВИДАЛИТИ
1. У [`src/engine/languages/manifests/syntax.rs`](../../src/engine/languages/manifests/syntax.rs) та [`src/engine/languages/dependency_registry.rs`](../../src/engine/languages/dependency_registry.rs):
   - **Видалити** рядкову індексацію `by_node_kind: HashMap<String, Vec<usize>>` для AST-диспатчу в гарячому циклі.
2. У [`src/engine/languages/python/projections.rs`](../../src/engine/languages/python/projections.rs) (рядки 260–420) та [`src/engine/languages/typescript/frameworks.rs`](../../src/engine/languages/typescript/frameworks.rs) (рядки 95–260):
   - **Видалити** повторні перевірки присутності пакетів через ітерацію імпортів на кожному виклику функції/декоратора, коли бітова маска файлу не містить бітів відповідного пакета.

#### 2.3. Що ДОДАТИ / ЗАМІНИТИ
1. У [`src/engine/languages/manifests/syntax.rs`](../../src/engine/languages/manifests/syntax.rs) додати в скомпільований `SyntaxRule` / `CompiledRuleSet`:
   - `pub(crate) import_mask: u64` (обчислений через [`package_mask`](../../src/engine/linker/interner.rs) під час компіляції маніфесту);
   - `pub(crate) kind_table: Box<[SmallVec<[u16; 2]>; 512]>` — плоска таблиця, індексована за `tree_sitter::Language::id_for_node_kind(kind, true) as usize`.
2. У [`src/engine/ast/visitor.rs`](../../src/engine/ast/visitor.rs) (`SinglePassExtractor`):
   - Підтримувати `file_import_mask: u64` у `FileContext` (оновлювати при кожному `imports` референсі через `package_mask(module_head)`).
   - На кожному вузлі брати `let kind_id = node.kind_id() as usize;` і звертатися до `kind_table[kind_id]` за O(1), пропускаючи правило за 1 такт, якщо `(self.file.import_mask & rule.import_mask) != rule.import_mask`.

#### 2.4. Як ПЕРЕВІРИТИ
- `cargo test --test languages profiles::` — золотий снепшот `tests/languages/fixtures/three_tier/pre_refactor.json` збігається 1-в-1.
- `cargo test --test languages manifests::` — усі 27 тестів фреймворк-паків проходять.

---

### Блок 3: Реальне підключення `LinkerInterner` (`SymbolId(u32)`, `PathId(u32)`) у таблиці лінкера

#### 3.1. Для чого це робити
Зараз [`src/engine/linker/interner.rs`](../../src/engine/linker/interner.rs) існує ізольовано (`#[allow(dead_code)]`), а лінкер у [`src/engine/linker.rs`](../../src/engine/linker.rs), [`src/engine/linker/semantic_context.rs`](../../src/engine/linker/semantic_context.rs), [`src/engine/linker/framework_relations.rs`](../../src/engine/linker/framework_relations.rs) та [`src/engine/linker/named_bindings.rs`](../../src/engine/linker/named_bindings.rs) створює мільйони тимчасових `String` та `BTreeSet<String>` ключів (`(String, String)`, `OccurrenceKey`), витрачаючи зайвий CPU та пам'ять під час фази лінкування. (`libc::malloc_trim(0)` на фазових переходах обов'язково **залишаємо**, щоб разом із інтернером ще сильніше знизити піковий RSS).

#### 3.2. Що ВИДАЛИТИ
1. У [`src/engine/linker/interner.rs`](../../src/engine/linker/interner.rs):
   - **Видалити** `#[allow(dead_code)]` з усіх методів `LinkerInterner`, `SymbolId`, `PathId`.
2. У [`src/engine/linker/framework_relations.rs`](../../src/engine/linker/framework_relations.rs):
   - **Видалити** зберігання клонованих `String` та `BTreeSet<String>` у `OccurrenceValue = (String, BTreeSet<String>, bool, bool)` (рядки 32–38), замінивши `BTreeSet<String>` для цільових вузлів на `SmallVec<[SymbolId; 2]>` або інтерновані ID.
3. У [`src/engine/linker/semantic_context.rs`](../../src/engine/linker/semantic_context.rs):
   - **Видалити** рядкові ключі `(String, String)` у внутрішніх таблицях `ComponentEnvironment`, `CallableAliases` та `RegistryBindings`, замінивши їх на `(PathId, SymbolId)`.

#### 3.3. Що ДОДАТИ / ЗАМІНИТИ
1. У [`src/engine/linker.rs`](../../src/engine/linker.rs) (`link_compact_with_typed_registry`):
   - Ініціалізувати `let interner = interner::LinkerInterner::from_catalog(&all);` на старті фази лінкування.
   - Передати `interner: &LinkerInterner<'a>` у `ResolutionInput`, `semantic_context::Context`, `PreparedSemanticFacts::build` та `named_bindings`.
2. Перевести ключі пошуку в `PreparedSemanticFacts`, `OccurrenceTable` та `semantic_context::Context` на `(PathId, SymbolId)` (`Copy`, 8 байтів разом).

#### 3.4. Як ПЕРЕВІРИТИ
- `rg "allow\(dead_code\)" src/engine/linker/interner.rs` — **0 збігів**.
- `cargo clippy --all-targets --all-features -- -D warnings` — **0 попереджень про невикористані методи `LinkerInterner`**.
- `cargo test --test linker` — усі 102 тести проходять.

---

### Блок 4: Прибирання подвійного зберігання JSON у `nodes.details` в SQLite

#### 4.1. Для чого це робити
У поточному пайплайні ([`src/db/writer/ingest.rs`](../../src/db/writer/ingest.rs) та [`src/db/writer.rs`](../../src/db/writer.rs)) кожен вузол має `node.details`, куди під час запису в таблицю `nodes` через `encode_facts(node, flows, semantic)` серіалізуються повні JSON-масиви `value_flow` (16 масивів) та `semantic_facts`. Водночас:
- для інкрементального дельта-реіндексу файлові факти все одно зберігаються окремо в таблиці `local_facts` (бінарний Zstd-блоб);
- жоден MCP-інструмент (`code_map_search`, `code_map_inspect`, `code_map_impact`, `code_map_explain`, `code_map_tests`) ніколи не читає `value_flow` чи `semantic_facts` із колонки `nodes.details` таблиці `nodes`!

Вони використовують у `nodes.details` лише навігаційні скалярні поля (`signature`, `visibility`, `return_type`, `param_types`, `doc`, `invariants`, `overload`, `is_stub`, `is_static`, `method_kind`, `macro_kind`).

#### 4.2. Що ВИДАЛИТИ
1. У [`src/db/writer.rs`](../../src/db/writer.rs):
   - **Видалити** проміжний буфер `PreparedCatalogStorage::details_json: Vec<String>`, який тримає в RAM сотні мегабайтів JSON-рядків для всіх вузлів перед вставкою в SQLite.
2. У [`src/core/typed_facts.rs`](../../src/core/typed_facts.rs):
   - Розділити публічну серіалізацію `Facts` (для unit-тестів AST та `local_facts`) і серіалізацію колонки `nodes.details` для таблиці `nodes` у SQLite, якщо тести не вимагають присутності `value_flow` безпосередньо в `SELECT details FROM nodes` (перевірити контракти у `tests/core/fact_persistence.rs` та `tests/commitment_integrity/`).
   - Якщо `seal::file_domain_leaves` хешує `details` для домену `nodes`, обчислювати хеш та `nodes.details` напряму з `DetailsView` потоково у `persist_files` без подвійного збереження у `PreparedCatalogStorage`.

#### 4.3. Як ПЕРЕВІРИТИ
- `cargo test --test core` (зокрема `fact_persistence` та `build_query_roundtrip`).
- `cargo test --test commitment_integrity` та `cargo test --test incremental`.

---

### Блок 5: Де-блоат `resolution.rs`, `value_flow.rs`, `semantic_context.rs` та уніфікація фреймворк-екстракторів (`projections.rs`, `render_context.rs`, `typescript/frameworks.rs`)

#### 5.1. Для чого це робити
Це головне джерело роздуття гілки `022`:
- [`src/engine/linker/resolution.rs`](../../src/engine/linker/resolution.rs) (2,805 рядків) складається з **однієї функції `resolve` на 2,645 рядків**, де один і той самий код ланцюжкового резолву полів (`for field in fields.split('.')`) і виклику `lookup_member` скопійований двічі (рядки 1089–1136 та 2008–2080).
- [`src/engine/linker/value_flow.rs`](../../src/engine/linker/value_flow.rs) (3,558 рядків) містить `ScopeTypeEnv`, який взагалі не підключений у `lookup_tracked`, плюс 540 рядків unit-тестів наприкінці файлу.
- [`src/engine/languages/python/projections.rs`](../../src/engine/languages/python/projections.rs) (1,153 рядки), [`src/engine/languages/python/render_context.rs`](../../src/engine/languages/python/render_context.rs) (1,223 рядки) та [`src/engine/languages/typescript/frameworks.rs`](../../src/engine/languages/typescript/frameworks.rs) (1,529 рядків) вручну реалізують одні й ті самі 4 операції (`EmitRoute`, `EmitEdge`, `BindReceiverType`, `ProjectContextKey`).

#### 5.2. Що ВИДАЛИТИ та РЕФАКТОРИТИ
1. **У [`src/engine/linker/resolution.rs`](../../src/engine/linker/resolution.rs) (Ціль: з 2,805 до ≤ 1,600 рядків)**:
   - **Видалити дубль** рядків 1089–1136 та 2008–2080, винісши єдину функцію `resolve_chained_member_target(receiver, member_chain, owner, path, at, value_flow, semantic_context, active_profile)`.
   - **Видалити дубль** класифікації `origin` для вбудованих типів (`logging.Logger`, `sqlite3.Connection`, `Web.Response`, `typed_dom_receiver`), винісши в `builtin_member_origin(receiver, active_profile, default_origin)`.
   - Декомпозувати 2,645-рядкову `resolve` на 5 компактних функцій:
     1. `resolve_import_reference` (рядки 280–915)
     2. `resolve_configured_and_template_reference` (рядки 920–1240)
     3. `resolve_explicit_hint_reference` (рядки 1245–1705)
     4. `resolve_receiver_and_member_reference` (рядки 1820–2150)
     5. `resolve_lexical_and_module_reference` (рядки 2155–2700)
2. **У [`src/engine/linker/value_flow.rs`](../../src/engine/linker/value_flow.rs) (Ціль: з 3,558 до ≤ 2,400 рядків)**:
   - **Підключити `ScopeTypeEnv::resolve_fast`** безпосередньо на початок `ValueFlowIndex::lookup_tracked`: якщо змінна не має локальних переприсвоєнь (`!self.has_local_write(owner, name)`) і має явну анотацію типу в сигнатурі або прямий конструктор у скоупі — повертати тип за O(1) без обходу графа досяжності `reaching` та епох `ReadTrace`.
   - **Винести 540 рядків unit-тестів** (рядки 3019–3558) із продакшн-файлу `src/engine/linker/value_flow.rs` в інтеграційний тестовий модуль `tests/linker/`.
   - Злити дубльовані гілки обчислення колекцій та JS/Python викликів у `evaluate_tracked` та `build_initialized`.
3. **У [`src/engine/linker/semantic_context.rs`](../../src/engine/linker/semantic_context.rs) (Ціль: з 3,522 до ≤ 2,300 рядків)**:
   - Скоротити 530-рядковий `prepare_component_environments` (рядки 473–1003) та 550-рядковий `visible` (рядки 1204–1754), об'єднавши спільні перевірки видимості імпортів і модульних скоупів для Python/TS/Rust.
4. **У [`src/engine/languages/python/projections.rs`](../../src/engine/languages/python/projections.rs), [`src/engine/languages/python/render_context.rs`](../../src/engine/languages/python/render_context.rs) та [`src/engine/languages/typescript/frameworks.rs`](../../src/engine/languages/typescript/frameworks.rs) (Ціль: сумарно -800..1,100 рядків)**:
   - Винести спільні примітиви емісії `SemanticFact` (`EmitRoute`, `EmitEdge`, `BindReceiverType`, `ProjectContextKey`) та нормалізації HTTP-методів / шаблонів шляхів у [`src/engine/languages/manifests/semantic.rs`](../../src/engine/languages/manifests/semantic.rs), видаливши їхні копії з `projections.rs`, `render_context.rs` та `typescript/frameworks.rs`.

#### 5.3. Як ПЕРЕВІРИТИ
- `wc -l src/engine/linker/resolution.rs src/engine/linker/value_flow.rs src/engine/linker/semantic_context.rs src/engine/languages/python/projections.rs src/engine/languages/python/render_context.rs src/engine/languages/typescript/frameworks.rs` — перевірити зменшення сумарного обсягу щонайменше на **2,500–3,500 рядків**.
- `cargo test --test linker --test languages` — 100% тестів проходять.

---

### Блок 6: Підняття точності індексації (Resolution Health із 18.5% до ≥ 65% на самому репозиторії `contextunity-forge-mcp`)

#### 6.1. Для чого це робити
Живий запуск `code_map_overview` на самому репозиторії `contextunity-forge-mcp` (259 Rust-файлів) показує:
- **Всього внутрішніх посилань**: `58,442`
- **Resolved**: лише `10,819` (**18.5%**)
- **`unresolved_receiver_type`**: `13,634`
- **`unknown`**: `32,381`

Аналіз нерозв'язаних посилань у Rust-коді репозиторію виявляє **4 конкретні системні прогалини**:

1. **Прогалина №1 — Розрив ланцюжків типів на Rust `?` (`try_expression`), `.unwrap()`, `.expect()`, `.as_ref()`, `.as_mut()`, `.as_deref()`, `.cloned()`, `.copied()` (~8,500 із 13,634 `unresolved_receiver_type`)**:
   - У Rust майже кожен другий виклик виглядає як `let conn = reader::open(...)?; conn.prepare(...)` або `let node = map.get(k).unwrap(); node.kind()`.
   - Зараз `rust/value_flow.rs` бачить тип повернення `Result<Connection, Error>` або `Option<&Node>`, але коли на ньому стоїть оператор `?` (`try_expression`) або метод `.unwrap()` / `.expect(...)` / `.ok()?` / `.as_ref()`, екстрактор і `ValueFlowIndex` перетворюють тип на `Unknown`! У результаті всі наступні виклики `conn.prepare()`, `tx.execute()`, `node.kind()` падають у `unresolved_receiver_type`.
2. **Прогалина №2 — Кваліфіковані модульні виклики `module::func()`, `super::func()`, `Self::assoc_fn()`, `Type::assoc_fn()` (~18,000 із 32,381 `unknown`)**:
   - У Rust виклики виду `commitments::hash(...)`, `writer::build(...)`, `Self::decode(...)`, `super::semantic::ValueFlowFacts::...` записуються з виразом `commitments::hash` або `Self::decode`.
   - У `resolution.rs` та `rust/linker.rs` для виразів із `::`:
     - префікс `Self::method` не підставляє поточний тип `impl Foo` для статичних асоційованих функцій, якщо немає `self` параметра;
     - односегментний модульний префікс `mod_name::func()` (коли `mod mod_name;` оголошений у батьківському `mod.rs` / `lib.rs` або імпортований через `use crate::...::mod_name;`) не резолвиться у файл модуля `mod_name.rs` / `mod_name/mod.rs`!
3. **Прогалина №3 — Транзитивний граф `pub use` / `use super::*;` / групових імпортів (`ModuleExportGraph`) (~9,000 із 32,381 `unknown`)**:
   - Коли файл робить `use super::*;` (як у 45 файлах цього репозиторію, наприклад `use super::*;` на початку `src/engine/languages/rust.rs` або `src/db/writer/ingest.rs`), символи з батьківського модуля та його `use`-імпортів не підтягуються для звичайних типів, констант і асоційованих викликів `Type::method()`.
4. **Прогалина №4 — Методи параметрів із посиланнями/зрізами та полями `self.field.method()` (~4,000 із 13,634 `unresolved_receiver_type`)**:
   - Коли тип поля структури записаний як `pub(crate) facts: Facts` або `Arc<LanguageContext>`, звернення `self.facts.nodes` або `self.language_context.rules()` не завжди знаходить тип поля, якщо структура визначена в іншому файлі або через `Deref` (`impl Deref for TypedFacts { type Target = Facts; }`).

#### 6.2. Що ДОДАТИ / ЗАМІНИТИ (Точні місця в коді)
1. **Розгортання `Result<T, E>` / `Option<T>` через `?`, `.unwrap()`, `.expect()`, `.as_ref()`, `.as_deref()`, `.cloned()`, `.copied()`**:
   - У [`src/engine/languages/rust/value_flow.rs`](../../src/engine/languages/rust/value_flow.rs):
     - Під час екстракції `try_expression` (`expr?`) не відкидати вираз як `Unknown`, а зберігати внутрішній вираз або прозоро розгортати конструктор/виклик;
     - Для методів-розгортачів (`.unwrap()`, `.expect()`, `.as_ref()`, `.as_mut()`, `.as_deref()`, `.cloned()`, `.copied()`, `.ok()`, `.into_iter()`, `.iter()`) на виразі `recv.unwrap()` зберігати тип приймача `recv`, розгортаючи generic-обгортку `Result<T, E> -> T` та `Option<T> -> T` у [`src/engine/linker/value_flow.rs`](../../src/engine/linker/value_flow.rs) (`return_type_tracked` / `evaluate_tracked`) та [`src/engine/languages/rust.rs`](../../src/engine/languages/rust.rs) (`return_type_name`).
2. **Резолв `Self::assoc_fn()`, `Type::assoc_fn()` та `mod_name::func()` у Rust**:
   - У [`src/engine/languages/rust/linker.rs`](../../src/engine/languages/rust/linker.rs) та [`src/engine/linker/resolution.rs`](../../src/engine/linker/resolution.rs):
     - Коли вираз має вигляд `Self::member`, знаходити найближчий enclosing `impl` вузол для `owner` (через `impl_target_by_owner`) і резолвити `member` як асоційовану функцію/метод цього типу;
     - Коли вираз має вигляд `Prefix::member` або `a::b::member`:
       1. Перевіряти, чи `Prefix` є типом у поточному модулі / імпортах / `use super::*;`, і шукати `member` у `RustMembers` цього типу;
       2. Якщо `Prefix` — це підмодуль (`mod prefix;` у поточному чи батьківському файлі або імпортований модуль), переходити за шляхом модуля у `RustForwarding` і шукати `member` серед публічних функцій/типів цього модуля.
3. **Повне транзитивне замикання `use super::*;` та `pub use` у `RustForwarding`**:
   - У [`src/engine/languages/rust/linker.rs`](../../src/engine/languages/rust/linker.rs) (`RustForwarding::build`):
     - Замикати `use super::*;` та `pub use ...::*;` на всі символи й імпорти батьківського модуля (включно з ланцюжками `child.rs -> mod.rs -> parent.rs`), щоб файли з `use super::*;` бачили всі типи, структури та функції батьківського модуля.
4. **Підтримка `impl Deref<Target = T>` для Rust-структур**:
   - У [`src/engine/languages/rust.rs`](../../src/engine/languages/rust.rs) (`RustMembers`):
     - При зустрічі `impl std::ops::Deref for Foo { type Target = Bar; }` (або `impl Deref for Foo`) записувати `deref_targets: HashMap<TypeName, TargetTypeName>`, щоб звернення `typed_facts.nodes` на `TypedFacts` автоматично знаходило поля та методи `Facts`!

#### 6.3. Як ПЕРЕВІРИТИ (Критерій приймання Блоку 6)
1. Прогнати всі існуючі тести Rust-лінкера:
   ```bash
   cargo test --test linker
   ```
   *(Увага: перевірити, щоб жоден негативний тест на кшталт `unknown_rhs_remain_unresolved` або `rust_trait_methods_require_visible_proven_traits` не зламався — розгортати тільки доведені статичні типи `Result<T, E>` / `Option<T>` / `Self` / видимі модулі).*
2. Зібрати релізний бінарник і перебудувати індекс самого репозиторію:
   ```bash
   cargo build --release
   ./target/release/contextunity-forge-mcp index build
   ```
3. Перевірити `code_map_overview` (або SQL-запит до `.forge/code-map.sqlite`):
   ```sql
   SELECT status, count(*) FROM resolution_coverage GROUP BY status;
   ```
   - **Цільовий показник**: `Resolution Health` зростає з **18.5% (10,819 / 58,442)** до **≥ 60–65%+ (35,000+ resolved)**, а `unresolved_receiver_type` падає з `13,634` щонайменше втричі.

---

### Блок 7: Зниження тиску на алокатор (`Allocation Pressure Diet` — мінус 2M+ алокацій на холодному білді)

#### 7.1. Для чого це робити
Під час холодного білду 4,000 файлів (~80,000 `Node` і ~350,000 `Reference`) потоки екстракції, кешування та лінкування роблять мільйони короткоживучих алокацій у купі (від 8 байтів до 256 KB), які створюють конкуренцію за арени `glibc` між потоками. Зменшення кількості алокацій безпосередньо знижує CPU на `malloc`/`free` та піковий RSS.

#### 7.2. П'ять конкретних механізмів зниження тиску на алокатор

1. **Ліквідація подвійного клонування `facts_to_dto` при записі в `ExtractionCache` ([`src/db/writer/cache.rs`](../../src/db/writer/cache.rs))**:
   - **Що видалити**: функцію `facts_to_dto(facts)` (рядки 478–545), яка робить повний `.clone()` усіх вузлів, ребер, референсів, документації та потоків у проміжний `FactsDto` перед записом у `Vec<u8>`.
   - **Що додати**: прямий прохід `BinaryEncode::encode(facts, &mut payload)` за посиланням `&TypedFacts` (0 проміжних клонів).

2. **Перевикористання воркер-локальних буферів (`source_buf`, `encode_buf`, `Parser`, `zstd::bulk::Compressor`) у [`src/engine/pipeline.rs`](../../src/engine/pipeline.rs), [`src/core/semantic.rs`](../../src/core/semantic.rs) та [`src/db/writer.rs`](../../src/db/writer.rs)**:
   - **Що видалити**:
     - По-файлове створення `let mut source = String::new();` у `extract_candidate` (`src/engine/pipeline.rs:447`);
     - По-файлове створення `tree_sitter::Parser::new()` у C-купі на кожен файл;
     - По-файлові виклики `zstd::stream::encode_all` / `decode_all` (`src/core/semantic.rs:68`, `src/db/writer.rs:64`), які щоразу алокують і звільняють `ZSTD_CCtx` (~256 KB на виклик).
   - **Що додати**:
     - Структуру `WorkerScratch` (або `thread_local!` буфери воркера), яка тримає `source_buf: String` (`.clear()` між файлами), `encode_buf: Vec<u8>`, кеш `tree_sitter::Parser` по мові та багаторазовий `zstd::bulk::Compressor` / `Decompressor`.

3. **Компактизація `ValueFlowFacts` із 432 байтів до ~120 байтів ([`src/core/semantic.rs`](../../src/core/semantic.rs))**:
   - **Що змінити**: зараз структура `ValueFlowFacts` містить **16 полів `Vec<...>`** (`16 * 24 = 384` байти + `return_type`), хоча для 95% звичайних функцій заповнені лише `bindings`, `fields`, `immutable_initializers`, `branch_joins`.
   - **Що додати**: винести 12 рідкісних фреймворк/шаблонних векторів (`symbol_attachments`, `factory_results`, `named_bindings`, `endpoint_requests`, `component_macros`, `rule_origins`, `component_configurations`, `template_contexts`, `type_only_imports`, `annotation_references`, `vue_slots`, `collection_elements`) у вкладений `pub framework: Option<Box<FrameworkScopeFacts>>` (з прозорими аксесорами або `#[serde(flatten)]`), зменшивши базовий розмір кожного `ValueFlowFacts` у 3.5 раза.

4. **Винесення `bindings`, `rebindings`, `param_types` із `node.details: serde_json::Value` у типізований сайдкар ([`src/core/models.rs`](../../src/core/models.rs), [`src/engine/ast/visitor.rs`](../../src/engine/ast/visitor.rs), [`src/engine/linker.rs`](../../src/engine/linker.rs))**:
   - **Що видалити**: запис `node.details["bindings"] = json!(...)`, `node.details["rebindings"] = json!(...)` та `node.details["param_types"] = json!(...)` під час AST-екстракції, де кожен ідентифікатор загортається в окремий `serde_json::Value::String` всередині `BTreeMap<String, Value>`, а потім на вході лінкера знову парситься.
   - **Що додати**: зберігати `bindings: Vec<String>`, `rebindings: Vec<String>`, `param_types: Vec<(String, String)>` у типізованому сайдкарі `TypedFacts` (або безпосередньо на `Node`), залишаючи `node.details = Value::Null` для простих вузлів (0 алокацій `BTreeMap` для звичайних функцій/методів).

5. **Усунення дубльованих рядкових алокацій `kind` / `language` / `path` / `source` ([`src/core/models.rs`](../../src/core/models.rs))**:
   - Для полів із фіксованим словником (`Node.kind`, `Node.language`, `Reference.kind`, `Edge.kind`, `Edge.confidence`) та повторюваних у межах файлу/скоупу ідентифікаторів (`Node.path`, `Reference.source`) мінімізувати клонування через інтернування в лінкері (`PathId`, `SymbolId`) та спільні буфери під час екстракції.

#### 7.3. Як ПЕРЕВІРИТИ
- Заміряти піковий RSS (`peak_rss_bytes` / `/usr/bin/time -v`) та час `extraction_ms` + `link_ms` + `sqlite_ingest_ms` на холодному білді до і після застосування Блоку 7.
- Усі 17 доменних Merkle-зобов'язань (`cargo test --test commitment_integrity`) та тести пам'яті (`cargo test --lib db::writer::memory_budget_tests`) мають проходити на 100%.

---

## 4. Порядок виконання та контрольні точки (`git stash` / коміти)

Щоб жоден крок не зламав робочий стан і ми бачили точні метрики після кожного блоку:

1. **Крок 1 (Блок 6 — Resolution Health 18.5% → 65%+)**:
   - Спочатку знімаємо поточний baseline із `.forge/code-map.sqlite` (`SELECT status, count(*) FROM resolution_coverage GROUP BY status`).
   - Вносимо зміни в `src/engine/languages/rust.rs`, `src/engine/languages/rust/value_flow.rs`, `src/engine/languages/rust/linker.rs` та `src/engine/linker/resolution.rs`.
   - Проганяємо `cargo test --test linker`, перебудовуємо індекс репозиторію і звіряємо нову таблицю `resolution_coverage`.
2. **Крок 2 (Блок 1 + Блок 7.1–7.3 — Декларативний `BinaryCodec`, видалення DTO з `cache.rs` / `writer.rs`, воркер-буфери та компактизація `ValueFlowFacts`)**:
   - Замінюємо ручну серіалізацію на `BinaryEncode`/`BinaryDecode` макроси без проміжних `*Dto` клонів, додаємо перевикористання буферів у воркерах та стискаємо `ValueFlowFacts`.
   - Перевіряємо `wc -l` (мінус ~850 рядків) та `cargo test --lib --test core --test commitment_integrity`.
3. **Крок 3 (Блок 2 + Блок 3 — `node.kind_id(): u16` таблиця диспатчу + підключення `LinkerInterner` у лінкер)**:
   - Переводимо AST-диспатч правил на `kind_id: u16` + `file_import_mask: u64`.
   - Підключаємо `LinkerInterner` (`SymbolId`, `PathId`) у `linker.rs`, `semantic_context.rs`, `framework_relations.rs`.
   - Перевіряємо `cargo clippy --all-targets --all-features -- -D warnings` та `cargo test --test languages --test linker`.
4. **Крок 4 (Блок 4 + Блок 5 + Блок 7.4 — Де-блоат `resolution.rs`, `value_flow.rs`, `semantic_context.rs`, `projections.rs`, `render_context.rs`, `typescript/frameworks.rs` та сайдкар `bindings`/`rebindings`)**:
   - Підключаємо `ScopeTypeEnv::resolve_fast` у `ValueFlowIndex::lookup_tracked`, виносимо 540 рядків unit-тестів із `value_flow.rs` у `tests/linker/`.
   - Декомпозуємо 2,645-рядкову `resolution::resolve` і прибираємо дубльовані блоки обходу полів.
   - Зводимо дубльовані процедурні правила у `projections.rs`, `render_context.rs`, `typescript/frameworks.rs` до спільних хелперів у `manifests/semantic.rs`.
   - Проганяємо повний `cargo clippy` + `cargo test` + фінальний замір `wc -l`, `peak_rss_bytes` та `Resolution Health`.

## 5. Рев’ю виконання (2026-10-24)

Початковий стан повністю відновлено; backup stash збережено:
`f20275fa5dd082f68fafaa3d79ce6f2ae343c365`. Початковий план вище не змінено.
За вказівкою власника тести виконуються після всіх реалізаційних і рев’ю фаз.
Forge task store повідомляє `unsupported tasks schema`; його не змінювали.

| Блок | Код / фактичний результат |
| --- | --- |
| 1 | Реальні clone DTO та ручні record codecs видалено; спільні primitive/struct/enum codecs, 16-vector macro, optional merges і V2/V3 framing. Ціль ≤2650 LOC не досягнута. |
| 2 | Прямі grammar-sized kind_id таблиці; порядок правил/grammar isolation збережено. File-mask AND відхилено. |
| 3 | Один borrowed interner; PathId/SymbolId у Prepared, Context, NamedBindings, ResolutionInput/value queries, occurrence owners/targets і позиційних таблицях. Source position/relation/mode збережено. |
| 4 | Public/navigation views уже розділені; буфер єдиної per-file navigation-проєкції залишено. |
| 5 | Прибрано тотожні field-walk, Web/DOM origin, collection/async-return, provider/prefix, origin-конструктори, 55 записів edge/coverage, borrowed node lookups і receiver scope walk. Три public-seam тести перенесено. П’ять окремих resolution-функцій не залишено; цілі скорочення LOC не досягнуті. |
| 6 | Proven wrapper adapters, declared modules, bounded transitive/lexical globs і custom Deref з guards для visibility/unknown/ambiguity/cycle/own members/std identity. Ціль ≥65% ще не підтверджена. |
| 7 | Worker source/codec buffers, warm parsers HTML/Vue, reusable Zstd contexts; optional12-vector box; native bindings/rebindings/ordered parameter Vec-pairs. Public JSON/V3/raw malformed/order збережено. Заміри очікуються. |

### Відхилені приписи та конкретні причини

- **1:** окремі flow/type/semantic binary codecs у baseline відсутні; `CacheFactsDto`,
  `NodeDto` та ручний generic record encoder були й видалені. Заміна JSON-value wire
  для типізованих полів на інший layout порушила б сумісність `MAGIC_V3`.
- **2:** literal512 менший за деякі grammar symbol counts; застосовано фактичний
  розмір grammar. File-import AND не дорівнює workspace-dependency OR та втрачає
  правила без файлових imports і з пізніми imports.
- **3:** project/domain/pack/rule/literal не є Node identity; довільна заміна всіх
  ключів на8байт губить provenance/позицію. Інтернування саме по собі не доводить
  існування owner; malformed owner зберігає стару unresolved coverage.
- **4:** prepared strings містять одну navigation-проєкцію одного файла після
  видалення structured details, а не дубль повного value-flow JSON каталогу.
  Їх видалення лише переносить/повторює матеріалізацію або змінює lifecycle.
- **5:** resolve_fast викликає lookup_tracked назад; literal fast-path рекурсує,
  bypass reaching/ReadTrace губить source-order/invalidation proof.
- **5:** спроба п’ятифункційного розбиття збільшила resolution.rs2868→4223LOC
  (+1355) без видалення policy-коду. Спробу відкинуто й точно відновлено стан
  перед нею. Спільні доказані dedup-зміни до цього стану збережено.
- **5:** private reducer tests не перенесено через потребу розкривати API.
- **5:** злиття Rust module/lexical проходів відхилено: interleaving змінює
  результат cross-file owner collisions. Початковий двофазний порядок відновлено.
- **6:** as_deref не доводиться: type_expr_inner стирає reference_type, хоча
  Wrapper і &Wrapper мають різні Deref::Target. Спробу adapter видалено.
  iter/into_iter/cloned/copied не є blanket payload identity.
- **7:** ~120B/3.5× не заявляється: baseline ValueFlowFacts виміряно704B;
  збережені common fields уже виключають literal target. details=Null загалом
  губить потрібні signature/visibility/doc поля. TLS buffers утримують max
  capacity; виграш RSS/часу без виміру не заявляється.

### Перевірка

Незалежне source review виправило lexical-glob/semantic-import регресії,
public-module reexport admission, std/core glob shadow і Python empty-box
алокації. Vec-pair cleanup перевірено разом із V3 codec consumers.
Проміжні test chains зупинені; вони не є фінальним доказом поточного коду.
Фінальні commands/counts, LOC і self-index/RSS receipts будуть дописані тут.

### 5.2.4 focused recheck

Framework projection execution, HTTP/path normalization, immutable projection-rule
preparation, and data-only API proof share `languages/manifests/semantic.rs`.
Language adapters retain AST extraction and source-specific lexical/import proof.
Against preserved stash `f20275f`: the three named extractors lose 816 lines;
all eight affected owner/adapter files together lose 82 lines. Tracked `src` still grows
by 405 lines overall, so the requested overall reduction remains unmet.
Independent source reviews pass for binding `adac0df59287b5faf6766ea1ea10fa4eccf8b99dbc73244f4e9f800abbd66b86`.
No build or test runs cover this final source. Existing production-seam assertions remain
applicable; direct malformed-render-key and invalid-method/`//` endpoint cases were not found.

### Блоки 1 і 5: повторна звірка LOC-цілей

- **1:** за збереженим stash три файли мали 3,510 рядків; зараз мають 3,424 (`-86`),
  до межі 2,650 бракує 774 рядки. DTO та ручні record codecs справді видалені.
  Решта різниці не відповідає видаленому boilerplate: codec/sidecar шлях і обробка
  `TypedFacts` залишаються потрібними. Без доведеного видалення поведінки ця межа
  недосяжна; її не зараховано.
- **5:** stash мав 13,790 рядків у шести файлах; після видалення 41 рядка невикористаного
  `ScopeTypeEnv` залишилося 12,652 (`-1,138`), тобто ще 1,362 до мінімуму 2,500.
  Три екстрактори дають `-816`. Три linker-файли мали 9,885 рядків у stash, а їхні
  індивідуальні caps сумарно дорівнюють 6,300: це вимагає видалити 3,585 рядків ще
  до скорочення екстракторів, що вже перевищує верхню агреговану ціль 3,500.
  Від поточних розмірів до індивідуальних caps лишається ще 3,263 рядки.
- Повторна перевірка живих розмірів спростовує наведені в плані обсяги двох методів
  `semantic_context.rs`: `prepare_component_environments` має 411 рядків, `visible` —
  24. У `value_flow.rs` unit-тести займають 292 рядки, а перенесення приватних reducer
  перевірок потребувало б розкриття внутрішнього API. Розбиття `resolve` на п'ять
  функцій вже пробували: воно додало 1,355 рядків без видалення логіки й було скасоване.
- `ScopeTypeEnv` / `resolve_fast` видалено: пошук посилань знайшов лише визначення,
  а запропонований шлях повертався в `lookup_tracked` і обходив `ReadTrace`.
  `has_local_write` залишено, оскільки його викликає `resolution.rs`.
- Отже, caps блоків 1 і 5 залишаються невиконаними; вище зафіксовано конкретні
  помилкові приписи/оцінки, а не підміну скорочення переміщенням коду.

### Manifest execution: production source recheck

- У `fastmcp.rs` обробник споживав загальний `DecoratedRegistration`, але зберігав
  внутрішній ключ `tool_decorators` та ID з префіксом `mcp-tool`. Модуль і ключ
  перейменовано за операцією, ID зроблено `tool-registration`; конфігурований
  `provider` та перевірку Python AST/lexical identity залишено.
- `compose_django_route_includes` та delta closure працювали від загального
  `RouteIncludeRule`/`include_module`; Python module identity обмежує цей синтаксис.
  Імена функції та evidence більше не називають Django. Правило включення та
  обмеження обходу залишені без зміни.
- Пошук посилань не знайшов старих runtime імен `fastmcp::`, `mcp-tool`,
  `tool_decorators`, `compose_django_route_includes` чи `django_route_include_closure`.
  `git diff --check` чистий. Фінальні тести/build залишені до завершення всіх фаз.

### Framework manifests: raw section removal

- `FrameworkManifest` no longer accepts or stores the top-level `receivers`,
  `builtins`, `filters`, and `routes` string sections. Their string-to-rule lookup
  helpers and linker consumers are removed. Equivalent template symbols and
  implicit receiver names use typed `syntax_rules` operations with rule-level
  activation; bundled packs, user examples, and the schema documentation use that
  form.
- Manifest contract cases accept typed TOML/YAML/YML rules and reject each removed
  top-level section. Verification remains pending until implementation phases are
  complete, as directed by the owner.

### Current source and LOC recheck (2026-10-24)

- Live counts against the preserved baseline: Block 1 is 3,408/3,510 LOC (-102),
  still 758 above its 2,650 cap. Block 5 is 12,680/13,790 LOC (-1,110), still
  1,390 short of its minimum 2,500-line reduction. The earlier review counts are
  stale; these are the current file counts.
- The tracked diff from `HEAD` is +15,421/-10,152 LOC (net +5,269 across 110
  tracked files; untracked files are excluded). This is not an overall LOC
  reduction.
- Tracked `src/` alone is +12,252/-6,825 LOC (net +5,427 across 73 source
  files). The code diff also grows overall.
- Exact AST searches find no `project_i18n` or `project.i18n` string in
  `src/engine`. The `django` and `fastapi` literals in `interner.rs` are below
  its `#[cfg(test)]` module; the example manifest in `dependency_registry.rs`
  is likewise below its test module. They are fixtures, not production pack
  selection.
- Keep the `lang-html` and `lang-vue` gates in `ast/mod.rs`: they dispatch to
  optional language grammars. `lang-vue` enables TypeScript and HTML grammars;
  ADR 0013 explicitly retains Vue as a language profile. These gates do not
  select framework packs.
- Generic linker `vue` checks refer to the Vue language profile and SFC template
  scope semantics. A blanket conversion to all `template_scope` nodes would also
  change temporal lookup for embedded HTML JavaScript scopes, which use the same
  node kind but have different source-order semantics. Keep that distinction
  until a manifest-backed or typed grammar fact exists; do not invent one here.
- Final tests/build remain deferred until implementation phases are complete,
  per owner instruction.

### Independent Sol high recheck and follow-up edits (2026-10-24)

- **1:** DTO and manual record cleanup is present. A positional layout is incompatible
  with the existing `MAGIC_V3` wire; a declarative adapter preserving its JSON-value
  tags/order/raw fallback remains possible but is not implemented or proven. Current
  three-file total is 3,408 LOC, 758 above the cap. `decode_prepared_storage` is now
  only reusable Zstd framing plus the generic storage codec; deleting its behavior
  would break staged ingest, while inlining only moves code. Final V3 wire evidence
  is still missing.
- **2:** fixed 512 slots cannot cover the admitted C++ grammar (534 symbols including
  aliases); the existing grammar-sized checked tables are the correct dispatch form.
  Blanket online `file_import_mask` AND is unsound for workspace activation, import-free
  rules, and imports discovered later. Added a completed-file negative filter to
  TypeScript `api_proven`; exact scope, alias, uniqueness, and type-only checks remain.
  Python's source-order frontier and candidate index remain unchanged pending a safe
  frontier-local optimization. Tests are pending.
- **3:** malformed owner interning must not imply a declaration exists; unresolved
  coverage remains before/after validation as before. Position, relation, and scope
  dimensions remain distinct. The review's broad rejection of all non-node interning
  is unsupported, but `ComponentScope`, named-binding, callable/result, and occurrence
  expression strings still contain cloneable dimensions. These are not yet reduced.
- **4:** source review confirmed the same navigation JSON was serialized once for
  prepared node leaves and again for SQLite. Node leaves are now produced in
  `persist_files_impl` from the exact JSON bytes bound to SQLite; prepared storage
  retains only document/shared-owner leaves. Original numeric row IDs remain assigned
  in source order; hashing processes each owner's nodes by canonical node ID. Full
  flow/semantic details remain excluded from navigation JSON. Commitment/persistence
  tests are pending.
- **5:** verified equivalent provider selection now lives in one shared linker helper;
  the duplicate type-only/export/Python guard is computed once before wildcard/named
  import handling. Existing field-walk and Web/DOM origin dedup remain. Broad builtin
  unification would change Python evidence. The five-function split, `resolve_fast`,
  remaining private test relocation, and LOC cap remain incomplete or rejected for
  source-order/ReadTrace/API reasons; collection/async/platform merges already exist.
- **6:** `?`, `unwrap`, `expect`, `ok`, `as_ref`, and `as_mut` preserve their proven
  contracts. Blanket identity remains invalid for `as_deref`, `iter`, `into_iter`,
  `cloned`, and `copied`: current facts erase reference shape and iterator item type.
  No adapter was added without that evidence; health and final linker tests remain
  unverified.
- **7:** optional boxing of 12 rare vectors and typed native binding sidecars are
  present. The 120-byte claim and blanket `details = Null` are invalid. Worker-local
  buffers are bounded by function/worker lifetime, but reusable cache payload and
  per-file `encode_facts` Zstd contexts remain. No capacity threshold or performance
  claim is added without measurement; the final RSS/time/allocation measurements and
  commitment/memory tests remain pending.
- No tests, builds, benchmarks, or self-index were run during this review phase.

### Поточні невиконані приписи та звірка LOC (2026-10-24)

- **Блок 1:** прямі `BinaryEncode`/`BinaryDecode` для `TypedFacts`, `MAGIC_V3`, SHA-256
  framing і видалення clone DTO реалізовані. Поточна сума трьох файлів — 3,416 LOC
  проти 3,510 у початковому зрізі; зменшення становить 94 LOC, до межі 2,650 бракує
  766. Тести перевіряють round-trip, але golden-перевірки байтової сумісності з
  початковим V3 payload немає.
- **Блок 2:** рядковий AST dispatch замінений grammar-sized `kind_id` таблицями.
  Literal 512 непридатний для граматик із понад 512 символами. Загальний
  source-order `file_import_mask` AND не застосований: він пропускав би правила без
  файлових imports та імпорти, виявлені пізніше. Є лише завершено-файловий негативний
  фільтр у TypeScript; еквівалентна оптимізація Python не доведена.
- **Блок 3:** інтернер підключений до linker resolution, bindings і occurrence
  owners/targets; occurrence expression зберігається як `SymbolId`. Рядкові виміри
  лишаються в component scope, callable/result catalog та частині named-binding
  таблиць. Перед переведенням потрібні IDs усіх вимірів і збереження origin/position.
- **Блок 4:** `PreparedCatalogStorage::details_json` видалений; navigation JSON
  серіалізується один раз у `persist_files`, а ті самі байти використовуються для
  node commitment і SQLite. Навігаційна проєкція не містить повних flow/semantic facts.
- **Блок 5:** шість файлів мають зараз 12,633 LOC проти 13,790 у початковому зрізі
  (−1,157); для мінімального скорочення 2,500 бракує 1,343 LOC. Поточні розміри
  linker-файлів: `resolution.rs` 2,641, `value_flow.rs` 3,338,
  `semantic_context.rs` 3,575. До індивідуальних меж 1,600/2,400/2,300 бракує
  загалом 3,254 LOC. Виконання всіх трьох меж разом із уже досягнутим скороченням
  екстракторів на 826 LOC вимагало б сумарного скорочення приблизно 4,411 LOC — це
  суперечить агрегованій верхній межі 3,500 LOC. Повторний field walk, частина
  origin/provider logic та дубльовані обчислення вже об'єднані. П'ятифункційний
  розподіл `resolve` і `resolve_fast` не застосовані: перший додає код без видалення
  policy, другий рекурсує через `lookup_tracked` та обходить `ReadTrace`. Приватні
  reducer tests лишаються на місці; перенесення вимагало б розкрити внутрішній API.
- **Блок 6:** доведені `?`, `unwrap`, `expect`, `ok`, `as_ref` та `as_mut` адаптери,
  qualified/module resolution, bounded globs/reexports і `Deref` реалізовані.
  `as_deref`, `iter`, `into_iter`, `cloned`, `copied` лишаються без адаптерів, бо
  факти стирають reference shape та iterator item type. Resolution Health і
  self-index ще не перевірені.
- **Блок 7:** optional boxing 12 рідкісних векторів, typed bindings/rebindings/
  `param_types` sidecar, worker-local source/codec buffers, warm parsers і повторне
  використання Zstd contexts частково реалізовані. Cache payload і per-file
  `encode_facts` Zstd contexts ще не перевикористовуються; RSS/часові заміри не
  виконані.
- За вказівкою власника фінальні тести, build, self-index та виміри виконуються
  тільки після завершення всіх реалізаційних і ревью фаз.

### Focused linker target review (2026-10-24)

- `cargo test --test linker`: 97 passed, 8 failed. Failures are
  `rust_tuple_field_contract_changes_relink_nominal_consumers_like_cold_builds`,
  `python_declared_builtin_shadowing_relinks_consumers_like_cold_builds`,
  `rust_aliases_and_explicit_flow_preserve_types_without_lifetime_noise`,
  `rust_declared_fields_resolve_nominal_parameters_and_tuple_receivers`,
  `rust_generic_annotations_preserve_arguments_without_implicit_unwrapping`,
  `rust_lifetime_only_nominals_preserve_imported_receiver_contracts`,
  `rust_str_receivers_resolve_core_inherent_members_without_prelude_imports`, and
  `rust_smart_pointer_deref_dispatch_persists_exact_providers_and_boundaries`.
  They report Rust value-flow/receiver and Python builtin-shadowing mismatches;
  final gate and per-failure attribution remain pending.

### Повторна звірка manifest-маршрутизації та мовних швів (2026-10-24)

- Структурні пошуки у `src/engine/linker` не знаходять pack-ідентифікаторів у
  production dispatch і не знаходять `project_i18n`/`project.i18n`. Вибір правил
  лишається прив'язаним до типізованих manifest facts; `framework_relations.rs`
  лишається загальним інтерпретатором цих фактів.
- Три планові adapter-файли мають 3,079 LOC проти початкових 3,905 (−826), що
  проходить їхню ціль −800…−1,100 LOC.
- Поточна сума шести планових шляхів — 10,496 LOC проти базових 13,790 (−3,294):
  `resolution.rs` 2,628, `value_flow.rs` 2,126, `semantic_context.rs` 2,663,
  adapters 886/1,054/1,139. Агрегована ціль −2,500…−3,500 за цим переліком шляхів
  досягнута; індивідуальні caps для `resolution.rs` і `semantic_context.rs`
  перевищені на 1,028 та 363 LOC. Цей підрахунок не дорівнює видаленим рядкам:
  мовні child-модулі містять винесений код, тому сімейний net LOC треба рахувати
  окремо й не приписувати перенесення видаленню.
- У спільному `semantic_context.rs` мовні receiver/import правила винесені в
  Python, JavaScript/TypeScript і Rust modules. Точкові наявні перевірки:
  Python logging, optional typing, type-checking imports і receiver — пройшли;
  JS/TS `this` — 6 пройшли; Rust generic receiver — пройшов. Також пройшли
  `framework_manifest_accepts_generic_typed_rules_and_formats`,
  `framework_manifest_class_attributes_follow_effective_rules_and_delta` та
  `starlette_and_fastapi_pack_relations_are_exact`,
  `registered_django_template_symbols_use_loaded_project_library` та
  `django_named_url_references_resolve_only_to_unique_routes`. Тест
  `rust_str_receivers_resolve_core_inherent_members_without_prelude_imports`
  знову впав із тією самою невідповідністю, що вже була у попередньому
  `cargo test --test linker` (97/105). Нових тестів не додавали.
- Повний gate і сімейний net-LOC підрахунок не виконані; індивідуальні caps та
  Block 1 залишаються відкритими.

### Review: HTML template helper extraction rejected (2026-10-24)

- Moving HTML template rule selection and evidence into helper functions compiled,
  but grew `resolution.rs` plus `html/templates.rs` from 3,481 to 3,614 LOC (+133)
  while only wrapping existing behavior. The experiment was reverted and earns no
  LOC-reduction credit. Keep the current manifest-backed behavior until a change
  removes logic rather than adding an indirection layer.

### Review: TypeScript DOM helper removal rejected (2026-10-24)

- Forge removal proof missed the path-qualified call from
  `linker/typescript/semantic_context.rs`; the focused build caught it. Keep and
  re-export `typed_dom_property` and `typed_dom_receiver` from the DOM module.

### Review: Python projection helper removal rejected (2026-10-24)

- The indexed removal proof missed two calls to `imported_module_candidate`,
  including one from `python/mod.rs`; the focused build caught both. Keep it.

### Повторне рев'ю виконання та залишків універсальних правил (2026-10-24)

- Повторна перевірка ядра не знайшла порівнянь за framework/pack identity у
  production dispatch. `build.rs::generate_framework_manifests` формує каталог
  стандартних паків зі знайдених файлів ідентифікуючи їх за stem; workspace loader
  читає `.forge/frameworks/*.{toml,yaml,yml}` і зливає пак за тим самим stem.
  Екстрактори отримують API, поля, aliases і activation із типізованих правил, а
  окремі мовні модулі лише розбирають власну AST-граматику. AST-пошук не знайшов
  `project_i18n`; згадки `django`/`fastapi` у перевірених Rust-шляхах належать
  тестовим fixtures. Архітектурний огляд Grok 4.7 High ще очікується.
- У `build_language_context` semantic activation більше не припускає Python-family:
  family і project scope визначаються через language profile з самого typed
  operation. Поточні валідовані Python-операції зберігають ту саму активацію.
- У `resolution.rs` об'єднано ідентичні `RustReference`/`ConfiguredReceiver` гілки.
  Для HTML library load вибір першого matching manifest rule тепер виконується
  один раз і напряму емітить його provenance, без повторного пошуку через
  синтезований `template.library.*` рядок.
- Блок 5: рев'ю підтвердило, що §5.3 вимагає скорочення **щонайменше** на
  2,500–3,500 LOC, отже 3,500 не є верхньою межею і попередній висновок про
  суперечність caps був хибним. Після точкових видалень шість названих шляхів
  мають 10,444 LOC проти 13,790 у базовому зрізі (−3,346); цей підрахунок не є
  net-LOC усіх child-модулів. `resolve` досі не розділений на п'ять функцій,
  `resolution.rs` і `semantic_context.rs` усе ще перевищують індивідуальні caps.
- Блок 6: повторна перевірка встановила, що `as_deref`, `iter` та `into_iter`
  уже мають обмежені адаптери з доведеними типами; старий висновок, ніби вони
  відсутні через втрату форми reference/item, застарів. `cloned`/`copied` лишаються
  Unknown без Clone/Copy доказу. Існуюче Rust receiver очікування конфліктує зі
  збереженням reference shape; позитивних adapter-кейсів ще немає. Тести відкладені
  до завершення реалізаційних фаз.
- Блок 7: blanket `details = Null` лишається некоректним. Повторне рев'ю уточнило,
  що TLS буфери декодування в Rayon живуть у довгоживучих worker threads, тому
  попередня заява про обмеження їхнього життя функцією/екстракційним worker була
  неправильною. Перевикористання cache payload/frame і per-file Zstd contexts та
  заміри RSS/часу ще не виконані.
- Блок 4: додано вузьке відновлення `doc`/`docstring` із збережених catalog facts
  до навігаційного JSON; наявний persistence-контракт уже перевіряє docstring.
  Виконання тесту відкладене разом із рештою фінальних тестів.

### Manifest dispatch recheck (2026-10-24)

- AST searches over `src/engine` found no `project_i18n` literal. Framework pack
  names (`django`, `fastapi`, `gridviewspec`, and the other bundled pack stems)
  occur in test fixtures; they do not select production linker behavior. Standard
  pack IDs continue to come from manifest file stems, and workspace packs are
  loaded from files under `.forge/frameworks/`.
- `framework_rule_diagnostics` still used a fixed Python/TypeScript/JavaScript
  route allowlist and Python package-family activation. Route eligibility now
  comes from `LanguageProfile::route_extraction_enabled`; semantic eligibility
  follows each typed rule's language and that profile's package family.
- Python class-hierarchy checks and Python-backed template-library traversal
  remain grammar-owned paths. They use Python syntax facts and do not select a
  framework pack. No additional pack-specific dispatch was found in the reviewed
  production linkers or manifest interpreters.
- A read-only Cursor-Agent Grok 4.7 High architecture review timed out without
  returning findings. This review used AST matches and the current ADR 0013/0017
  boundaries. No test or build was run; final verification remains deferred.

### Five-step implementation review (2026-10-24)

- The extraction cache now accepts only the `FORGE-EXTRACTION-FACTS\0\x04` frame. The V2 encoder and JSON fallback, along with their compatibility tests, are removed. Unknown frames and checksum mismatches return a cache miss.
- `ValueFlowFacts` keeps the four common vectors and `return_type` inline; the twelve framework vectors are behind `Option<Box<FrameworkScopeFacts>>`. Its measured size is 152 bytes. Empty `node.details` maps compact to `Value::Null`.
- Replaced the unstable `&str::as_str()` calls with dereferences and added Rust wrapper member lookup through the Rust language profile.
- Removed `python/render_context.rs` and `typescript/frameworks.rs`. Python syntax operations use shared semantic manifest helpers; TypeScript object projections live in the projection modules. Strict Clippy completed without dead-code warnings.
- All ten `libc::malloc_trim(0)` call sites remain.
- Final gates passed: `cargo clippy --all-targets --all-features -- -D warnings`; `cargo test --lib` (104 passed); `cargo test --test linker` (105 passed); `cargo test --test languages` (275 passed).

#### LOC acceptance remains open

The worktree's `git diff --stat` reports 123 tracked files changed, 14,786 insertions, and 23,990 deletions (net -9,204 tracked lines). It omits untracked Rust modules, which currently contain 17,413 lines. Counting all Rust source files gives 108,239 LOC; the current `main` branch has 95,727 LOC, so this worktree is +12,512 LOC against `main`. The initial audit recorded +9,626 against `main`; the current snapshot is therefore 2,886 LOC larger than that recorded starting point. The tracked-only negative diff is not evidence of a complete source reduction.

Block 1's three-file total (`cache.rs`, `writer.rs`, `typed_facts.rs`) is 3,468 LOC, 818 above its 2,650 cap. The block 5 aggregate reduction also remains unproven after accounting for code moved into untracked child modules. Do not report the five-step LOC reduction or the full plan's LOC caps as complete; further in-scope deletion is required.

### Final verification refresh (2026-10-24)

- Re-ran the requested gates on the current worktree: strict Clippy passed; `cargo test --lib` passed (104), `cargo test --test linker` passed (105), and `cargo test --test languages` passed (275). The linker run includes `rust_generic_annotations_preserve_arguments_without_implicit_unwrapping`.
- Recounted the `libc::malloc_trim(0)` invariant: all ten call sites remain.
- Current `git diff --stat`: 123 tracked files changed, 14,786 insertions, 23,993 deletions (net -9,207 tracked lines). The stat excludes untracked files. Current Rust source is 108,229 LOC, including 17,406 LOC in untracked Rust modules; `main` has 95,727 Rust source LOC, so this worktree is +12,502 LOC against `main`.
- Block 1 remains 3,468 LOC, 818 above its cap. Block 5's family-wide net reduction remains unproven; moved code is not counted as deleted code. These broader LOC acceptance items are still open even though the five requested implementation steps and their test gates pass.

---

## 6. Архітектурний рефакторинг та ліквідація бойлерплейту (Закриття відкритого LOC-боргу)

### 6.1. Діагноз: чому виник розрив між планом і фактичним обсягом коду

Попередній етап декомпозиції застосував механічне розрізання коду (*code slicing*) замість спрощення архітектури:
1. **Бойлерплейт розрізання замість зменшення логіки**:
   - Функція `resolve_import_reference` у [`src/engine/linker/resolution/imports.rs`](../../src/engine/linker/resolution/imports.rs) приймає **10 параметрів** через `#[allow(clippy::too_many_arguments)]`.
   - Для повернення локального стану створено проміжний DTO `ImportBindings` (три хешмапи), що створює зайвий інтерфейсний клей.
   - Головний файл [`src/engine/linker/resolution/mod.rs`](../../src/engine/linker/resolution/mod.rs) залишився монолітом на **2,065 рядків** (усередині однієї паралельної Rayon-лямбди змішано імпорти, виклики, шаблони, роути та документацію).
2. **Роздуття Блоку 1 (`cache.rs`) на +818 рядків**:
   - Перехід на прямий бінарний кодек `MAGIC_V4` замість Serde DTO реалізовано через масивний ручний опис макросів `struct_codec!` для кожного типу фактів ядра. Це дало 0 heap-алокацій, але роздуло `src/db/writer/cache.rs` до 1,600+ рядків.

---

### 6.2. Покроковий план ліквідації бойлерплейту та реального скорочення коду

#### Крок 6.2.1: Патерн `LinkerContext` та `FileResolver` у `src/engine/linker/resolution/`
- **Що усунути**:
  - Прибрати функцію з 10 параметрами `resolve_import_reference` та `#[allow(clippy::too_many_arguments)]`.
  - Прибрати штучний контейнер `ImportBindings`.
- **Що зробити**:
  - Оформити стан розв'язання одного файлу як структуру `FileResolver<'a, 'file>`:
    ```rust
    pub(super) struct FileResolver<'a, 'file, F> {
        pub input: &'file ResolutionInput<'a, F>,
        pub path: &'file str,
        pub facts: &'a Facts,
        pub file_module: &'file str,
        pub graph: Graph,
        pub aliases: HashMap<&'file str, HashMap<&'a str, Vec<&'a Node>>>,
        pub external_aliases: HashMap<&'file str, ExternalAliases<'a>>,
        pub type_only_aliases: HashMap<&'file str, HashSet<&'a str>>,
        pub lookup_key: String,
    }
    ```
  - Перетворити обробку імпортів у методи `FileResolver` (або функції, що приймають `&mut FileResolver`). Усі потрібні залежності (`by_qual`, `dependency_registry`, `normalized_imports`) читаються напряму через `self.input`.
  - У результаті сигнатури скорочуються до 1 параметра (`&mut self`), проміжний контейнер `ImportBindings` зникає, а код читається лінійно.

#### Крок 6.2.2: Pass-Based Pipeline замість монолітної 2,000-рядкової лямбди `resolve`
- **Що усунути**:
  - Розбити 2,065-рядкове тіло паралельного замикання `all.par_iter().map(|(path, facts)| { ... })` у `resolution/mod.rs` на чіткі послідовні фази.
- **Що зробити**:
  - Розділити процес резолву файлу на 4 ізольовані фази:
    1. `phase_imports(resolver: &mut FileResolver)` — зв'язування імпортів (файл `resolution/imports.rs`, ~150 LOC).
    2. `phase_value_flow(resolver: &mut FileResolver)` — виклики методів, властивості, типи через `ValueFlowIndex` (~300 LOC).
    3. `phase_framework_projections(resolver: &mut FileResolver)` — роути, HTML-шаблони та компоненти (~200 LOC).
    4. `phase_documentation(resolver: &mut FileResolver)` — docstrings і Markdown-посилання (~100 LOC).
  - Головний файл `resolution/mod.rs` стає чистим координатором фаз розміром **< 500 рядків**.

#### Крок 6.2.3: Архітектурне скорочення Блоку 1 (`cache.rs` + `writer.rs` + `typed_facts.rs` $\le$ 2,650 LOC)
- **Діагноз проблеми**:
  Поточний обсяг трьох файлів становить **3,963 LOC** замість цільової межі **2,650 LOC** (+1,313 рядків перевищення). Причина — не складність алгоритму, а колосальний обсяг ручного бойлерплейту:
  1. Подвійна ручна серіалізація: один і той самий набір даних серіалізується вручну двічі (ручний бінарний курсор у `cache.rs` на ~1,000 рядків та 9 ручних `impl Serialize`/`Deserialize` у `typed_facts.rs` на ~500 рядків).
  2. "Химера" типізації: `Node` одночасно тягне типізований `native_semantic` і динамічний `details: serde_json::Value`, що змусило написати 80-рядковий монстр `encode_node_details` для побайтового злиття JSON та бінарних полів на льоту.
  3. Розбиття на 16 окремих векторів у `ValueFlowFacts`, що породило багаторівневий макро-бойлерплейт `for_each_scope_fact_vec!` та `scope_fact_keys!`.

- **Архітектурно правильні підпункти виправлення**:

  ##### 6.2.3.1. Десегрегація типізованого ядра від зовнішніх View (Separation of Core & Presentation)
  - **Що усунути**: Функцію `encode_node_details` (80 LOC) у `cache.rs` та ручний `impl Serialize for DetailsView` (100 LOC) у `typed_facts.rs`.
  - **Архітектурне рішення**:
    - Усередині двигуна (`engine`) та в кеші на диску оперувати виключно типізованими полями `NodeSemanticEntry` (`param_types`, `bindings`, `rebindings`).
    - Динамічний об'єкт `details` — це виключно шар представлення (Presentation View) для клієнтів MCP та CLI. Він формується назовні через легку проєкцію (DTO) лише при читанні або відповіді на запит.
    - Ефемерний бінарний кеш зберігає лише типізовану структуру `Node` без необхідності парсити, валідувати й мерджити сирий JSON `details`.

  ##### 6.2.3.2. Ліквідація ручного Serde-бойлерплейту (DTO з `#[derive(Serialize)]` замість 9 ручних `impl`)
  - **Що усунути**: Ручні реалізації `Serialize`/`Deserialize` для `DetailsView`, `NodeView`, `PublicNodesView`, `ParameterTypesView`, `SemanticState`, `FlowState`, `ClassicFlow`, `TypedFacts` (~350 LOC низькорівневих викликів `serialize_entry`, `serialize_map`) та функцію `FlowStore::details(&self, ...)` (60 LOC), яка клонувала `serde_json::Map` і матеріалізувала тимчасові `serde_json::Value` на кожен вузол.
  - **Архітектурне рішення та покрокова інструкція**:
    1. **Створення позиченого DTO `NodeDetailsDto<'node, 'flow>`**:
       - Структура тримає виключно позичені посилання (`&'node Node`, `&'flow FlowStore`, `Option<&'node SemanticState>`) без жодних heap-алокацій.
       - Реалізує `Serialize` напряму через `serializer.serialize_map(...)`, записуючи `bindings`, `rebindings`, `param_types`, `value_flow`, `semantic_facts`, `named_definition_uncertainties` одразу в потік виводу.
    2. **Оновлення `NodeView<'node, 'flow>`**:
       - Замінити `details: Value` на `details: NodeDetailsDto<'node, 'flow>`, ліквідувавши проміжне створення DOM-дерева `Value::Object`.
    3. **Видалення `FlowStore::details`**:
       - Замінити важку функцію побудови `Value` на нуль-алокаційний метод `flows.details_view(node, semantic) -> NodeDetailsDto<'node, 'flow>`.
    4. **Збереження wire-format**:
       - JSON-відповідь для MCP (`tools/call`) та CLI залишається на 100% сумісною і байт-у-байт ідентичною, але генерується стрімінгово без зайвих мільйонів heap-алокацій `serde_json::Value`.

  ##### 6.2.3.3. Уніфікація сховища фактів потоку значень (Tagged Storage замість 16 векторів і макросів)
  - **Що усунути**: Макроси `for_each_scope_fact_vec!` (lines 612–626), `scope_fact_keys!` (lines 628–636), ручні циклічні виклики підрахунку в `scope_fact_count` та роздуті методи `FlowStore` (~400 LOC).
  - **Архітектурне рішення**:
    - Замість 16 окремих полів `Vec<Fact>` у `ValueFlowFacts` та `FrameworkScopeFacts`, де кожна зміна тягне розширення макросу, структурувати факти через типізований перелік (Tagged Fact / Enum) або нормалізований каталог фактів.
    - `scope_fact_count` зводиться до звичайного виклику `len()`, а макрос `for_each_scope_fact_vec!` видаляється повністю.
    - Операції обходу, індексації та фільтрації фактів реалізуються через стандартні ітератори Rust замість макро-генерації коду.

  ##### 6.2.3.4. Компактизація та очищення бінарного кодека `cache.rs` та ліквідація JSON у `local_facts`
  - **Що усунути**:
    - Ручний 100-рядковий енкодер `serde_json::Value` (lines 339–404), невикористану гілку `default` у `struct_codec!`, та надлишкові перевантаження кодеків у `cache.rs`.
    - Непотрібну зворотну сумісність із JSON у SQLite `local_facts` ([`src/core/typed_facts.rs`](../../src/core/typed_facts.rs)): видалити fallback `serde_json::from_reader` у `decode_facts`.
  - **Архітектурне рішення**:
    - Звести виклики `struct_codec!` у серійну макрогрупу для доменних структур (`Edge`, `Reference`, `DocSection`, `Diagnostic`, `ScopeFact`).
    - Оскільки ефемерний кеш фактів екстракції (`MAGIC_V4`) та таблиця `local_facts` повністю відновлюються переіндексацією за 2–4 секунди, формат не потребує версіонування застарілих JSON-схем.
    - У `decode_facts`: якщо блоб не містить префіксу `LOCAL_FACTS_BINARY_MAGIC`, повертати помилку (fail-closed / cache-miss), що ініціює швидкий перепарсинг замість тягання застарілого JSON-рушія десеріалізації.
    - Скоротити `src/db/writer/cache.rs` з 1,660 LOC до **$\le$ 900 LOC**.
    - Скоротити `src/core/typed_facts.rs` з 1,640 LOC до **$\le$ 1,100 LOC**.

- **Критерій успіху Блоку 1**:
  - `src/db/writer/cache.rs` ($\le$ 900) + `src/db/writer.rs` ($\le$ 650) + `src/core/typed_facts.rs` ($\le$ 1,100) = **$\le$ 2,650 LOC**.

#### Крок 6.2.4: Table-Driven скорочення у `semantic_context.rs`
- **Що усунути**:
  - Процедурні перевірки методів стандартних бібліотек (Rust wrappers, Python builtins) всередині монолітного методу `lookup_member_mode`.
- **Що зробити**:
  - Звести пошук стандартних методів до методів профілю `LanguageProfile::builtin_member` та декларативних маніфестів.
  - Скоротити `semantic_context.rs` до цільового розміру **$\le$ 1,800 рядків**.

#### Крок 6.2.5: Архітектурна декомпозиція тестових монолітів без оверкіла (Modular Test Decomposition)
- **Діагноз проблеми**:
  У тестовій системі ядра двигуна утворилися гігантські монолітні файли по 1,500–3,600 рядків, у яких неможливо ефективно орієнтуватися і де дублюються сотні рядків однакових хелперів:
  1. [`tests/languages/manifests.rs`](../../tests/languages/manifests.rs) — **3,605 LOC** (усі 15 фреймворків в одному файлі!).
  2. [`tests/languages/typescript/frameworks.rs`](../../tests/languages/typescript/frameworks.rs) — **2,916 LOC** (NestJS, Nuxt, Vue в купі).
  3. [`tests/languages/html/framework_relations.rs`](../../tests/languages/html/framework_relations.rs) — **2,177 LOC** (HTML-зв'язки для всіх бекендів разом).
  4. [`tests/languages/html/templates.rs`](../../tests/languages/html/templates.rs) — **1,829 LOC**.
  5. [`tests/receivers/rust_value_flow.rs`](../../tests/receivers/rust_value_flow.rs) — **1,564 LOC**.
  6. [`tests/receivers/python_value_flow.rs`](../../tests/receivers/python_value_flow.rs) — **1,441 LOC**.
  7. Перші 200–250 рядків у кожному такому файлі — це скопійований однаковий SQL-бойлерплейт (`assert_edge_canonical`, `coverage_status`, `query_rows`, `write_files`).

- **Дворівнева архітектура тестів (Рішення БЕЗ оверкіла)**:
  - **Рівень 1: Бінарники Cargo (`tests/*.rs`)**:
    - Зберігати обмежену кількість кореневих виконуваних бінарників двигуна (рівно **3 бінарники**: `tests/linker.rs`, `tests/languages.rs`, `tests/framework_manifests.rs`). Це запобігає 15-хвилинній комбінаторній лінковці десятків окремих `.rs` файлів.
  - **Рівень 2: Внутрішня модульна декомпозиція (логічне розбиття по доменах та підсистемах)**:
    - Розбити кожен тестовий моноліт на тематично ізольовані підмодулі за природними доменними межами через стандартну модульну систему Rust (`mod`). Вони компілюються як один бінарник, але дають ідеальну ізоляцію, охайність і миттєвий точковий запуск.

- **Конкретний план модульного розбиття**:
  1. **Декомпозиція маніфестів (`tests/framework_manifests/`)**:
     - `support.rs` — винести спільні SQL-хелпери та асерти (`assert_edge_canonical`, `coverage_status`, `query_rows`), ліквідувавши дублювання в тестах.
     - Розбити 3,605-рядковий `manifests.rs` на окремі файли за фреймворками:
       - `django.rs` (~250 LOC — ORM, Model, View, uses_model)
       - `fastapi.rs` (~250 LOC — маршрути, DI, response_model)
       - `htmx.rs` (~200 LOC — `hx-get`, `hx-target`, `calls_endpoint`)
       - `alpine.rs` (~200 LOC — `$store`, `x-data`)
       - `nuxt.rs` (~250 LOC — pages, nitro, auto-imports)
       - `nestjs.rs` (~250 LOC — контролери, декоратори, модулі)
       - `vue.rs` (~200 LOC — Pinia, Composition API)
       - `gridviewspec.rs` (~150 LOC — таблиці, endpoint-зв'язки)
       - `activation.rs` (~150 LOC — валідація схем TOML та активація паків)
  2. **Декомпозиція HTML-шаблонів та зв'язків (`tests/languages/html/`)**:
     - Розбити `framework_relations.rs` (2,177 LOC) та `templates.rs` (1,829 LOC) на:
       - `html/django_templates.rs` (~350 LOC)
       - `html/jinja_templates.rs` (~300 LOC)
       - `html/htmx_relations.rs` (~350 LOC)
       - `html/alpine_relations.rs` (~300 LOC)
  3. **Декомпозиція TypeScript фреймворків (`tests/languages/typescript/frameworks/`)**:
     - Розбити 2,916-рядковий `frameworks.rs` на:
       - `nestjs.rs` (~600 LOC)
       - `nuxt.rs` (~600 LOC)
       - `vue_runtime.rs` (~500 LOC)
  4. **Декомпозиція потоку значень (`tests/receivers/`)**:
     - Розбити `rust_value_flow.rs` (1,564 LOC) на:
       - `rust/field_chains.rs` (~350 LOC — структури, кортежні ресівери, ланцюжки полів)
       - `rust/trait_dispatch.rs` (~400 LOC — статична й динамічна диспетчеризація трейтів)
       - `rust/deref_pointers.rs` (~300 LOC — `Box`, `Arc`, `&T`, `Deref`)
     - Розбити `python_value_flow.rs` (1,441 LOC) на:
       - `python/c3_mro.rs` (~400 LOC — C3 MRO, множинне успадкування)
       - `python/mixins.rs` (~350 LOC — домішки, декоратори методів)

- **Результат для розробника та CI**:
  - Нуль дублювання SQL-хелперів (економія ~1,500 рядків копіпасти).
  - Миттєвий точковий запуск будь-якого підмодуля за **0.05 секунди**:
    ```bash
    cargo test --test framework_manifests manifests::fastapi::
    cargo test --test framework_manifests manifests::dependencies::
    cargo test --test framework_manifests manifests::finite_operations::
    cargo test --test languages html::django_templates::
    cargo test --test languages typescript::frameworks::nuxt::
    cargo test --test linker typed_receivers::rust::field_chains::
    cargo test --test linker typed_receivers::rust::module_paths::
    cargo test --test linker typed_receivers::python::c3_mro::
    cargo test --test linker value_flow::
    ```
  - Час загальної лінковки не зростає, оскільки кількість важких бінарників у `tests/` залишається мінімальною (+1 для маніфестів).
  - Каталог тестів і команди запуску оновлено в [`tests/AGENTS.md`](../../tests/AGENTS.md) та [`docs/testing/README.md`](../testing/README.md).

#### Крок 6.2.6: Повна нормалізація кваліфікованих шляхів Rust (`super::`, `super::super::`, `crate::`, `self::`) та зняття штучних обмежень покриття
- **Діагноз проблеми**:
  У кодографі Forge виявлено критичний дефект втрати ребер викликів (`calls` edge loss): виклик `super::claim::claim_nested_gate(...)` у `src/engine/tasks/lifecycle.rs` не зв'язується з функцією `claim_nested_gate` у `claim.rs` (`callers: 0` у code map).
  Причини:
  1. Неправильна кваліфікація шляху: у `src/engine/linker/resolution/value_flow_phase.rs` вираз `super::claim::...` замінюється на `super.claim...`, після чого лінкер наївно конкатенує його до поточного модуля `file_module + ".super.claim..."` замість того, щоб піднятися по модульному дереву.
  2. Жорсткий фільтр однакового файлу: блок `candidates.retain(|n| n.path == *path ...)` безжально відкидає будь-якого кандидата з іншого файлу, якщо виклик не був попередньо імпортований через `use` (`imported.is_none()`).
  3. Відсутність підтримки асоційованих функцій через модуль (`super::module::Type::method()`): при спробі виклику статичного методу через відносний шлях лінкер не вміє розділити шлях модуля та ім'я типу.

- **Що зробити (повне поширення нормалізації)**:
  1. **Ітеративний резолвер модульних префіксів у `value_flow_phase.rs`**:
     - **Багаторівневі `super::` (`super::super::...`)**:
       ```rust
       if let Some(mut tail) = expr.strip_prefix("super::") {
           let mut module = file_module.rsplit_once('.').map_or("", |(p, _)| p);
           while let Some(rest) = tail.strip_prefix("super::") {
               module = module.rsplit_once('.').map_or("", |(p, _)| p);
               tail = rest;
           }
           format!("{module}.{}", tail.replace("::", "."))
       }
       ```
     - **Корінь крейта `crate::...`**:
       Розгортати відносно базового модуля крейта (`crate.src` або `src`), відкидаючи локальний `file_module`.
     - **Дочірні модулі `self::...`**:
       Розгортати відносно поточного `file_module` як `format!("{file_module}.{}", tail.replace("::", "."))`.
     - **Асоційовані методи типів через модульний шлях**:
       Якщо останній сегмент нормалізованого шляху або передостанній є типом (`struct`, `enum`), виконувати пошук асоційованої функції/методу в скоупі цього типу.

  2. **Корекція фільтра локального файлу (`n.path == *path`)**:
     - У блоці фільтрації кандидатів `candidates.retain(...)`: якщо вираз містить будь-який явний префікс міжфайлової адресації (`super::`, `crate::`, `self::`), або якщо повне ім'я (`qualname`) знайденого кандидата точно збігається з нормалізованим цільовим шляхом, **не відкидати кандидата за ознакою іншого файлу**.

  3. **Комплексні контрактні тести покриття**:
     - Додати до `tests/linker/method_semantics.rs` або `tests/receivers/rust_value_flow.rs` тести на всі форми адресації:
       1. Однорівневий відносний виклик: `super::sibling::func()`.
       2. Багаторівневий виклик: `super::super::ancestor::func()`.
       3. Абсолютний виклик крейта: `crate::subsystem::func()`.
       4. Дочірній виклик: `self::child::func()`.
       5. Модульно-кваліфікований статичний метод: `super::sibling::MyStruct::new()`.
     - Переконатися, що у згенерованому графі коду кожен із цих викликів має статус `resolved`, ребро `calls` та з'єднує правильні вершини.

---

### 6.3. Критерії приймання та верифікація

1. **Жорсткий ліміт LOC Блоку 1**:
   - `wc -l src/db/writer/cache.rs src/db/writer.rs src/core/typed_facts.rs` $\le$ **2,650 LOC**.
2. **Ліквідація Clipy-винятків за кількістю аргументів**:
   - Усунено `#[allow(clippy::too_many_arguments)]` у `resolution/imports.rs` та `resolution/mod.rs`.
3. **Збереження функціональних гейтів**:
   - `cargo clippy --all-targets --all-features -- -D warnings` — 0 warnings.
   - `cargo test --lib` — 104 passed.
   - `cargo test --test linker` — 105 passed.
   - `cargo test --test languages` та `cargo test --test framework_manifests` — сумарно 275 passed (100% збереження тестового покриття).
4. **Збереження інваріантів**:
   - Усі 10 викликів `libc::malloc_trim(0)` залишаються недоторканими на своїх місцях.
5. **Верифікація міжфайлових викликів (`super::`, `super::super::`, `crate::`, `self::`)**:
   - Контрактний тест доводить, що виклики всіх форм модульної адресації Rust (`super::sibling::func`, `super::super::ancestor::func`, `crate::path::func`, `self::child::func`) успішно зв'язуються в ребро `calls` і відображаються у графовому пульсі `callers >= 1`.

### Review finding: value-flow test relocation

- [`tests/AGENTS.md`](../../tests/AGENTS.md) keeps private reducer implementation checks beside their source and public value-flow cases in [`tests/linker/value_flow.rs`](../../tests/linker/value_flow.rs).
- Five public value-flow cases now live in `tests/linker/value_flow.rs`. The private merge-summary and borrowed-key checks remain in [`src/engine/linker/value_flow/tests.rs`](../../src/engine/linker/value_flow/tests.rs), as required by the source boundary.
- The strict-order helper test is removed after its boundary cases were added to the public replay test. The unit-level awaited-promise test is removed because [`awaited_promise_parameters_and_aliases_survive_index_readback`](../../tests/linker/method_semantics.rs) proves that contract through the persisted linker path.
- The §4 Step 4 estimate of about 540 LOC is not a reason to move private implementation checks across the test boundary. The public cases are partitioned; do not claim that the literal LOC transfer target is complete.

### Review finding: §6.2.5 test decomposition diagnosis

- A complete declaration search across the six named test monoliths found `write_files`, `query_rows`, `coverage_status`, and `assert_edge_canonical` only in `tests/languages/manifests.rs`. The HTML, TypeScript, and Rust files do not repeat this preamble; Python has different persistence helpers. The asserted ~1,500 LOC copied-helper saving is unsupported and is not claimed.
- `tests/languages/manifests.rs` contains 27 tests, including table-driven bundled-pack activation, a mixed cross-pack persistence test, and a generic finite-operation test. Only the FastAPI relation contract is a standalone pack-specific test. The planned nine per-pack module names do not match the current test boundaries; tests remain once in truthful activation, FastAPI, cross-pack, generic-operation, dependency, discovery, CommonJS, and delta modules. No empty pack files or synthetic per-pack tests were added.

### Review finding: §6.2.5 moved-suite coverage and size exceptions

- All 24 HTML tests and 16 relation helper/scenario functions were moved once into HTML template, browser-template, manifest-relation, and framework-relation modules. The combined HTMX/Alpine persisted contract remains one test because both assertions share its indexed fixture.
- All 28 TypeScript cases were preserved: 27 framework tests now live under `frameworks::{nestjs,nuxt,vue_runtime}`, and the parser-selection test moved to `typescript::grammar`. The 426-LOC integrated Nuxt/Vue/Pinia test and 404-LOC composable-return test remain single cases.
- All 27 Rust receiver tests were preserved across field, trait, pointer, builtin, value-flow, and `module_paths` modules. `module_paths` also retains the §6.2.6 cross-file path contract. The largest file is `deref_pointers.rs` at 358 LOC.
- All 34 Python receiver tests were preserved across ten factual groups. The current suite has no C3-precedence or mixin-class test; `c3_mro.rs` contains only the closest existing inheritance baselines, and no empty `mixins.rs` was created.
- `cross_pack_relations.rs` (719 LOC) remains one integrated persisted contract over its shared fixture. The finite-operation suite is now split across a sequential test wrapper, a shared lifecycle runner (`operations.rs`, 727 LOC), five operation case modules, and separate callable-import and linked-workspace target-context workflows. It preserves the original test name/cfg, five case rows, and 66 assertion invocations. The grouped files total 2,003 LOC; splitting is strictly organized along clear logical domain and contract boundaries rather than artificial line-count caps.

### Review finding: revised storage recommendations

- Cache codec tests use private framing and binary traits. They now live in the source-owned child module [`cache/tests.rs`](../../src/db/writer/cache/tests.rs); moving them into an external integration target would require widening private APIs. `cache.rs` contains the 253-line production module.
- [`NodeSemanticEntry::ingest`](../../src/core/typed_facts/semantic_store.rs#L55) moves admitted fields out of `Node.details` with `remove` into `native_semantic`; the binary `Node` codec persists that typed state, and the borrowed [`NodeDetailsDto`](../../src/core/typed_facts/presentation.rs#L235) projects it back when requested. Restoring duplicate JSON fields would reverse the memory reduction, and the proposed destructive `drain` is not present in the current implementation.
- [`decode_facts`](../../src/core/typed_facts.rs#L49) already rejects non-binary local-facts data. The generic `serde_json::Value` codec in [`binary.rs`](../../src/core/typed_facts/binary.rs#L162) remains necessary for arbitrary `Node.details` and preserved invalid raw evidence; it is not a JSON-cache fallback. The [`enum_codec!`](../../src/core/typed_facts/binary.rs#L254) identity arm is used by unvalidated enum codecs, and `FrameworkFact` already uses the tuple-enum codec at [binary.rs](../../src/core/typed_facts/binary.rs#L346).
- The remaining `SemanticState`/`FlowState` serialization in [presentation.rs](../../src/core/typed_facts/presentation.rs#L22) preserves original raw values and classic field ordering around the derived borrowed `NodeDetailsDto`; deleting it without equivalent views would change observable output. [`FlowStore::merge_uncertainties`](../../src/core/typed_facts/flow_store.rs#L174) enforces bounded groups, unknown-key collapse, and invalid-carrier fault behavior, so replacing it with a generic `BTreeSet` helper does not support the proposed 250-line reduction.

### Review finding: Block 1 file-path LOC criterion

- The three paths named by §6.2.3.4 currently total 1,269 LOC: `cache.rs` 253, `writer.rs` 678, and the `typed_facts.rs` facade 338. The explicit ≤2,650 sum passes. The implementation family, including `cache/tests.rs` and the four typed-facts child modules, is 3,975 LOC; moving those files does not remove their lines. `writer.rs` is 28 lines above its descriptive 650-line estimate, but no artificial split was added to meet that per-file estimate.

### Review finding: §3 numeric-key recommendations

- The outer `PackageExports` map is keyed by the static language-profile id used to select the correct export index for a file. Its inner path and exported-symbol keys are already `PathId` and `SymbolId`. Replacing the outer key with `PathId` would conflate indexes for different language profiles, so that recommendation is not applied.
- Named bindings and component scope lookups already use interned IDs. The proposed path-to-`HashSet` imported-name catalog is not present in `factories.rs`; there is no current table to replace with `SmallVec`.

### Review finding: §6.2.2 value-flow phase estimate

- The four-phase coordinator is in place and `resolution/mod.rs` is 370 LOC. Source review found and extracted the HTML template-symbol helpers, computed-call handling, candidate-join reporting, and inherited-method edge emission into their existing language/domain owners.
- `phase_value_flow` remains 1,819 LOC, with 257 LOC in its two helper modules. The remaining lexical/import/typed fallback is one ordered resolver over shared scope, alias, and candidate state; reducing the phase to the approximate 300 LOC estimate would require another context container or a change to fallback ordering. This estimate remains unmet and is recorded rather than claimed complete.

### Review finding: cold coverage comparison on the control and library workspaces

- A one-repeat cold comparison used baseline binary `aa479392061bc671a038c8a7b8206e1283532b3fa3e4668da12d25d40f0f0c5f` and candidate binary `1a6e4b30cc4a4ea29df47b55af936a0271bce1b0be2b6625f8cf9e2af82d5624` against the same workspace contents.
- On `commerce-release-update`, resolution health excluding external imports rose from 74.1252% to 74.6627% and unresolved references fell by 1,005. However, the coverage comparison failed: it found 44 lost code edges (15 `calls_endpoint`, 29 `renders_template`) and 48 lost resolved references, with 875 resolved references added. No code files, code nodes, or test-reached production nodes were lost. The candidate contains no `renders_template` edges. These missing PIM relations require follow-up; the higher aggregate resolution percentage does not establish preservation.
- On `library`, no canonical code edges, resolved references, files, nodes, or test-reached production nodes were lost. Unresolved references increased by three and resolution health changed from 41.3613% to 40.7216%. The coverage policy passed, while full semantic parity did not.
- Each comparison used one repeat. Treat wall-time deltas as observations, not stable performance measurements.

### Review finding: all-target incremental test failures

- `cargo test --all-targets` exited 101 in `tests/incremental`; 28 incremental cases passed and three failed. A serial rerun with `cargo test --test incremental -- --test-threads=1` reproduced all three, ruling out test-thread interference.
- [`body_edits_refresh_owner_without_relinking_consumers_and_line_moves_relink`](../../tests/incremental/resolution_identity.rs#L81) reported 32 affected owners for a one-file body edit where the contract expects one.
- [`vue_setup_body_edits_refresh_calls_and_preserve_consumer_contracts`](../../tests/incremental/scope.rs#L484) recorded `defineOptions` as unresolved instead of the manifest-backed Vue macro built-in.
- [`source_delta_relinks_affected_docs_with_bare_suffix_references`](../../tests/incremental/doc_parity.rs#L70) produced different cold and delta output roots for the same source snapshot.
- These failures remain open; no test expectations were weakened. The full all-target gate is not passing.

