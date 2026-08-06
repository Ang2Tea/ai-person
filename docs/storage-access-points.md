# Точки обращения к хранилищу

Аудит всех мест, где код читает/пишет одно из пяти файловых хранилищ:
`working_memory.json` (буфер), `diary/*.md` (дневник), `commitments/{chat_id}.md`,
`insights.md`, `system_prompt.md`. Составлено как база для будущего переноса
на единый `ObjectStorage`/`MemoryBackend` (S3/Postgres) — см. обсуждение плана
развития проекта.

## При старте

Один раз, в момент сборки зависимостей.

- [`bin/bot/src/main.rs:49-50`](../bin/bot/src/main.rs#L49-L50) — `LocalFileStorage::new` + `BufferStore::new` → внутри вызывается `storage.load()` ([`bot-core/src/adapters/local_file_storage.rs:26`](../bot-core/src/adapters/local_file_storage.rs#L26), `fs::read_to_string` всего `working_memory.json`).
- [`bin/bot/src/main.rs:52`](../bin/bot/src/main.rs#L52) — `fs::read_to_string(system_prompt_path())`.
- [`bin/bot/src/main.rs:56`](../bin/bot/src/main.rs#L56) / [`bin/admin/src/main.rs:93`](../bin/admin/src/main.rs#L93) — `fs::read_to_string(insights_path())` (в `admin` — только в ветке `Sleep`).
- [`bin/bot/src/main.rs:53-54`](../bin/bot/src/main.rs#L53-L54) / [`bin/admin/src/main.rs:55-56`](../bin/admin/src/main.rs#L55-L56) — `MemoryStore::new`/`CommitmentsStore::new` — сами конструкторы ничего не читают, только сохраняют путь.

## Периодически в фоне

- **Буфер** — [`bot-core/src/buffer.rs`](../bot-core/src/buffer.rs) (`spawn_flush_task`), раз в 30с: если были изменения — `storage.save()` → `fs::write`+`fs::rename` ([`local_file_storage.rs:42-43`](../bot-core/src/adapters/local_file_storage.rs#L42-L43)). При штатном завершении — ещё один принудительный `buffer.flush()` ([`bin/bot/src/main.rs:144`](../bin/bot/src/main.rs#L144), [`bin/admin/src/main.rs:88`](../bin/admin/src/main.rs#L88)).
- **Idle-extraction** — [`bot-core/src/idle_extraction.rs:40-41`](../bot-core/src/idle_extraction.rs#L40-L41) (раз в 5 минут): `buffer.chat_ids()`, `buffer.get()` по каждому чату → при готовности [`idle_extraction.rs:55`](../bot-core/src/idle_extraction.rs#L55) вызывает `memory::maybe_extract` (см. ниже).
- **Proactive** — [`bot-core/src/proactive.rs:54-55`](../bot-core/src/proactive.rs#L54-L55) (интервал из конфига): `buffer.chat_ids()`, `buffer.get()` — только на чтение, для выбора неактивного чата.
- **Ночная консолидация** — [`bot-core/src/consolidation.rs`](../bot-core/src/consolidation.rs), 3:00 локального времени:
  - [`consolidation.rs:92`](../bot-core/src/consolidation.rs#L92), [`consolidation.rs:105`](../bot-core/src/consolidation.rs#L105) — `memory.list_all()` (полное чтение директории дневника), несколько раз за прогон;
  - [`consolidation.rs:266`](../bot-core/src/consolidation.rs#L266), [`consolidation.rs:268`](../bot-core/src/consolidation.rs#L268) — `memory.append()`/`memory.remove()` при слиянии похожих фактов;
  - [`consolidation.rs:353`](../bot-core/src/consolidation.rs#L353) — `memory.remove()` при удалении нерелевантных (`prune_irrelevant`);
  - [`consolidation.rs:384`](../bot-core/src/consolidation.rs#L384) — `memory.remove()` при удалении устаревших (`remove_stale`);
  - [`consolidation.rs:292`](../bot-core/src/consolidation.rs#L292) — `read_system_prompt()` → `fs::read_to_string`, нужен как контекст для prune-промпта;
  - [`consolidation.rs:423-426`](../bot-core/src/consolidation.rs#L423-L426) — `write_insights()` → `fs::create_dir_all`+`write`+`rename`, пересборка `insights.md`.

## На каждый ход (сообщение/правка/реакция/проактивный ход)

Всё — в [`bot-core/src/bot.rs`](../bot-core/src/bot.rs).

- [`bot.rs:213`](../bot-core/src/bot.rs#L213) — `buffer.push()` входящего сообщения (`run_turn`).
- [`bot.rs:465`](../bot-core/src/bot.rs#L465) — `buffer.push()` исходящего ответа (`send_and_record`).
- [`bot.rs:215`](../bot-core/src/bot.rs#L215), [`bot.rs:249`](../bot-core/src/bot.rs#L249) — `buffer.get()`, сборка контекста хода (`run_turn`/`run_proactive`).
- [`bot.rs:298`](../bot-core/src/bot.rs#L298) — `commitments.get()` в `build_messages`, подмешивается в системный промпт (диск-чтение внутри [`commitments.rs:23`](../bot-core/src/commitments.rs#L23)).
- [`bot.rs:292`](../bot-core/src/bot.rs#L292) — `self.insights.read().await` в `build_messages` — не диск, `Arc<RwLock<Arc<str>>>` в памяти (обновляется консолидацией).
- [`bot.rs:338`](../bot-core/src/bot.rs#L338), [`bot.rs:376`](../bot-core/src/bot.rs#L376) — `memory.list_filtered()` + `memory.touch()` в `retrieve_relevant_facts`, автопоиск релевантных фактов по каждому входящему сообщению.
- [`bot.rs:235`](../bot-core/src/bot.rs#L235), [`bot.rs:272`](../bot-core/src/bot.rs#L272), [`bot.rs:485`](../bot-core/src/bot.rs#L485) — если превышен `token_threshold`, `maybe_spawn_extraction` асинхронно запускает [`memory::maybe_extract`](../bot-core/src/memory/extraction.rs):
  - [`memory/extraction.rs:74`](../bot-core/src/memory/extraction.rs#L74) — `buffer.get()`, снять транскрипт;
  - [`memory/extraction.rs:79`](../bot-core/src/memory/extraction.rs#L79), [`memory/extraction.rs:119`](../bot-core/src/memory/extraction.rs#L119) — `commitments.get()` + `commitments.set()`;
  - [`memory/write.rs:24`](../bot-core/src/memory/write.rs#L24), [`memory/write.rs:42`](../bot-core/src/memory/write.rs#L42) — `save_fact`: `memory.list_filtered()` (дедуп по чату) + `memory.append()` на каждый новый факт;
  - [`buffer.rs:151`](../bot-core/src/buffer.rs#L151) — `buffer.truncate_keep_last()`, обрезка буфера после извлечения.

## Инструмент `remember` (по решению модели)

- [`bot-core/src/tools/remember.rs`](../bot-core/src/tools/remember.rs) — тот же путь `save_fact`, что и выше: `memory.list_filtered()` + `memory.append()`.

## Инструменты чтения (по решению модели)

- [`bot-core/src/tools/list_known_chats.rs:51`](../bot-core/src/tools/list_known_chats.rs#L51) — `buffer.chat_ids()`.
- [`bot-core/src/tools/read_chat_history.rs:72`](../bot-core/src/tools/read_chat_history.rs#L72) — `buffer.get()`.
