# ai-chat-person

Telegram-бот на Rust с личностью, краткосрочной и долгосрочной памятью, работающий через
OpenAI-совместимый API Timeweb Cloud (`chat/completions` + `embeddings`) с tool-calling.

## Стек

`teloxide` (Telegram, long polling), `tokio`, `reqwest`, `serde`, `config` (TOML), `thiserror`,
`chrono`, `serde_yaml`.

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
у бота через `@BotFather` → `/setprivacy` → Disable.

## Структура

```
src/
  main.rs                — сборка зависимостей, запуск teloxide::repl
  bot.rs                 — ChatBot: цикл обработки сообщения + tool-calling
  chat_locks.rs           — per-chat мьютекс, сериализует обработку одного чата
  settings.rs             — конфиг из config.toml
  contracts.rs            — формат обмена с LLM (OpenAI-подобный)
  buffer.rs               — краткосрочная память (буфер переписки по чатам)
  memory/                 — долгосрочная память: факты, эмбеддинги, дедуп, поиск
  adapters/               — HTTP-клиент к Timeweb, файловое хранилище буфера
  tools/                  — инструменты модели (send_message, search_memory, remember, ...)
personalities/<name>/
  system_prompt.md        — системный промпт личности
  working_memory.json     — сериализованный буфер переписки
  diary/*.md              — факты долгосрочной памяти (по одному файлу на факт)
prompts/                  — служебные системные промпты, не привязанные к личности
  extraction_system.md    — роль модели при фоновом выделении фактов
  extraction_instruction.md — формат ответа при выделении фактов
config.toml               — путь к активной личности, модель, пороги памяти
```

`personalities/` и `.env` — в `.gitignore`, реальные диалоги и токены в репозиторий не попадают.

## Разработка

```
cargo check
cargo test
cargo clippy --all-targets -- -D warnings
```

