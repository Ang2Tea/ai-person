# ai-chat-person

Telegram-бот на Rust с личностью, краткосрочной и долгосрочной памятью, работающий через
OpenAI-совместимый API Timeweb Cloud (`chat/completions` + `embeddings`) с tool-calling. Реагирует
не только на текст, но и на опросы/дайсы/геолокацию/контакты/стикеры/подписи к медиа, а также на
правки сообщений и реакции других людей — всё проходит через один и тот же буфер и tool-calling
цикл.

## Стек

`teloxide` (Telegram, long polling), `tokio`, `futures`, `reqwest`, `serde`, `config` (TOML),
`thiserror`, `chrono`, `serde_yaml`.

## Запуск

1. `.env` в корне репозитория:
   ```
   BOT_TOKEN=<токен бота из @BotFather>
   TIMEWEB_KEY=<ключ Timeweb AI Gateway>
   RUST_LOG=warn,ai_chat_person=debug
   ```
2. `config.toml` в корне — уже есть рабочий пример (активная личность, модель, пороги памяти).
   Путь к конфигу можно переопределить переменной окружения `CONFIG_PATH`.
3. `cargo run`.

Если бот должен отвечать в группах, а не только в личных сообщениях — отключите privacy mode
у бота через `@BotFather` → `/setprivacy` → Disable. Чтобы видеть реакции на сообщения в группах —
бот должен быть администратором этой группы (иначе Telegram не присылает `message_reaction`
апдейты вообще, независимо от `allowed_updates`).

## Структура

```
src/
  main.rs                — сборка зависимостей, цикл поллинга (Message/EditedMessage/MessageReaction)
  bot.rs                 — ChatBot: разбор апдейтов, общий tool-calling цикл (run_turn/run_proactive)
  chat_locks.rs           — per-chat мьютекс, сериализует обработку одного чата
  consolidation.rs        — ночная консолидация дневника + генерация insights
  proactive.rs            — периодический воркер: даёт модели шанс написать первой
  idle_extraction.rs       — извлечение фактов по простою чата (не только по порогу токенов)
  settings.rs             — конфиг из config.toml
  contracts.rs            — формат обмена с LLM (OpenAI-подобный)
  buffer.rs               — краткосрочная память (буфер переписки по чатам)
  memory/                 — долгосрочная память: факты, эмбеддинги, дедуп, поиск
  adapters/               — HTTP-клиент к Timeweb, файловое хранилище буфера
  tools/                  — инструменты модели (send_message, send_reaction, search_memory, remember, ...)
personalities/<name>/
  system_prompt.md        — системный промпт личности
  insights.md             — генерируется ночной консолидацией, подмешивается в системный промпт
  working_memory.json     — сериализованный буфер переписки
  diary/*.md              — факты долгосрочной памяти (по одному файлу на факт)
prompts/                  — служебные системные промпты, не привязанные к личности
  extraction_system.md    — роль модели при фоновом выделении фактов
  extraction_instruction.md — формат ответа при выделении фактов
  consolidation_merge_system.md — роль модели при слиянии похожих фактов
  insights_system.md      — роль модели при генерации insights из публичных фактов
config.toml               — путь к активной личности, модель, пороги памяти
```

Раз в сутки в 3:00 по локальному времени сервера фоновая задача (`consolidation.rs`) сливает
похожие факты дневника в один (силами LLM), удаляет давно не использовавшиеся факты и
пересобирает `insights.md` из публичных фактов — обновление подхватывается ботом сразу, без
перезапуска.

С интервалом `[proactive].interval_minutes` (`config.toml`) фоновый воркер (`proactive.rs`)
выбирает случайный известный чат, в котором давно (`min_inactivity_minutes`) не было сообщений —
живой разговор в выборку не попадает — и с шансом `probability` даёт модели возможность написать
туда первой; решение писать или промолчать (`wait`) остаётся за моделью.

Факты в дневник извлекаются не только по накоплению `token_threshold` токенов в переписке, но и по
простою чата (`idle_extraction.rs`, `memory.idle_extraction_after_minutes`) — иначе для короткой
личной переписки факт мог бы очень долго не появляться в архиве и быть недоступным боту в других
чатах с тем же человеком.

`personalities/` и `.env` — в `.gitignore`, реальные диалоги и токены в репозиторий не попадают.

## Разработка

```
cargo check
cargo test
cargo clippy --all-targets -- -D warnings
```

