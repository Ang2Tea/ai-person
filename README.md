# ai-chat-person

Telegram-бот на Rust с личностью, краткосрочной и долгосрочной памятью, работающий через
OpenAI-совместимый API Timeweb Cloud (`chat/completions` + `embeddings`) с tool-calling. Реагирует
не только на текст, но и на опросы/дайсы/геолокацию/контакты/стикеры/подписи к медиа, а также на
правки сообщений и реакции других людей — всё проходит через один и тот же буфер и tool-calling
цикл.

## Стек

`teloxide` (Telegram, long polling), `tokio`, `futures`, `reqwest`, `serde`, `config` (TOML),
`thiserror`, `chrono`, `serde_yaml`, `tracing`/`tracing-subscriber`, `clap` (CLI `admin`-бинарника).

## Запуск

1. `.env` в корне репозитория:
   ```
   BOT_TOKEN=<токен бота из @BotFather>
   TIMEWEB_KEY=<ключ Timeweb AI Gateway>
   RUST_LOG=warn,ai_person=debug
   ```
2. `config.toml` в корне — уже есть рабочий пример (активная личность, модель, пороги памяти).
   Путь к конфигу можно переопределить переменной окружения `CONFIG_PATH`.
3. `cargo run -p app --bin bot`.

Если бот должен отвечать в группах, а не только в личных сообщениях — отключите privacy mode
у бота через `@BotFather` → `/setprivacy` → Disable. Чтобы видеть реакции на сообщения в группах —
бот должен быть администратором этой группы (иначе Telegram не присылает `message_reaction`
апдейты вообще, независимо от `allowed_updates`).

## Деплой и запуск на сервере (GitHub Actions + systemd)

Продакшн (`ai-person-vds`, `/root/ai-person-release`) больше не использует tmux — сборка
переехала в GitHub Actions (`.github/workflows/deploy.yml`), а запуск на сервере идёт через
systemd. Каждая личность — отдельный **инстанс шаблонного юнита**, а не отдельный tmux-сеанс.

### Как это работает

1. Push в `main` → GitHub Actions собирает статический musl-бинарник
   (`cargo build --release --target x86_64-unknown-linux-musl --features strict-messaging
   -p app --bin bot`) — сервер сам ничего не компилирует (слишком слаб, 1 vCPU/1GB RAM).
2. Готовый бинарник заливается на сервер как `bot.new`, затем по SSH: текущий `bot`
   бэкапится в `bot.prev`, `bot.new` атомарно переименовывается в `bot`
   (`mv`, не перезапись — иначе `ETXTBSY` на работающем процессе), и вызывается
   `/root/ai-person-release/deploy-restart.sh`, который перезапускает все инстансы личностей.
3. Секреты в `SSH_HOST`/`SSH_USER`/`SSH_KEY` (GitHub → Settings → Secrets and variables →
   Actions) — деплой заходит на сервер отдельным SSH-ключом, не личным.

### systemd: как это устроено

Один **шаблонный юнит** на все личности — `/etc/systemd/system/ai-person-bot@.service`.
Имя после `@` (например, `default` или `masha`) подставляется в юнит как `%i`:

```ini
[Service]
WorkingDirectory=/root/ai-person-release
EnvironmentFile=/root/ai-person-release/common.env   # общие TIMEWEB_KEY/RUST_LOG
EnvironmentFile=/root/ai-person-release/%i.env        # свой BOT_TOKEN на личность
Environment=CONFIG_PATH=/root/ai-person-release/config-%i.toml
ExecStart=/root/ai-person-release/bot
Restart=always
KillSignal=SIGINT   # обязательно — иначе systemd убьёт процесс SIGTERM'ом и пропустит флаш буфера
```

Сейчас реально запущены `ai-person-bot@default` (личность `default`, `config-default.toml`,
`default.env`) и `ai-person-bot@masha` (личность `masha`, `config-masha.toml`, `masha.env`).

**Полезные команды:**

| Команда | Что делает |
|---|---|
| `systemctl status ai-person-bot@default` | жив ли процесс, последние строки лога |
| `journalctl -u ai-person-bot@masha -f` | смотреть логи вживую (замена `tmux attach`) |
| `systemctl restart ai-person-bot@<имя>` | перезапустить один инстанс (флашит буфер через SIGINT) |
| `systemctl daemon-reload` | обязательно после правки файла юнита, до `restart` |

**Добавить новую личность:**
```
cd /root/ai-person-release
cp config-default.toml config-<имя>.toml
# в config-<имя>.toml поменять [personality] path = "personalities/<имя>"
echo "BOT_TOKEN=<токен из @BotFather>" > <имя>.env
chmod 600 <имя>.env
# дописать "ai-person-bot@<имя>.service" в цикл внутри deploy-restart.sh
systemctl enable --now ai-person-bot@<имя>
```

**Откат на предыдущую версию бинарника** (если деплой сломал прод):
```
cd /root/ai-person-release && mv bot.prev bot && ./deploy-restart.sh
```
`bot.prev` хранит только одну предыдущую версию — перезаписывается при каждом следующем деплое.

## Структура

Cargo workspace, разбитый на крейты по границам ответственности:

```
contracts/                — общие типы и трейты (Llm, Storage, Memory, Message, ToolCall, ...),
                             на них ссылаются все остальные крейты, не зная друг о друге напрямую
bot-core/                 — ядро бота, не привязанное к конкретному каналу/провайдеру
  src/bot.rs               — ChatBot: разбор апдейтов, общий tool-calling цикл
  src/chat_locks.rs         — per-chat мьютекс, сериализует обработку одного чата
  src/consolidation.rs      — ночная консолидация дневника + генерация insights
  src/idle_extraction.rs    — извлечение фактов по простою чата (не только по порогу токенов)
  src/scheduler.rs          — общий паттерн периодических фоновых задач (spawn_periodic)
  src/tools/                — инструменты модели, не завязанные на канал (remember, wait, get_current_datetime)
memory/                   — долгосрочная память: факты, эмбеддинги, дедуп, поиск, commitments
adapters/
  llm/timeweb/              — HTTP-клиент к Timeweb AI Gateway (chat/completions, embeddings, vision)
  storages/fs/               — файловая реализация Storage
  channels/telegram-bot/      — интеграция с Telegram (teloxide): диспетчер апдейтов, send_message/
                                send_reaction, проактивная рассылка (jobs/proactive.rs)
app/                      — сборка конкретных зависимостей и бинарники
  src/bin/bot.rs            — запуск бота (long polling)
  src/bin/admin.rs          — CLI для ручного запуска фоновых задач (extract/sleep), без Telegram
  src/settings.rs           — конфиг из config.toml
personalities/<name>/
  system_prompt.md        — системный промпт личности
  insights.md             — генерируется ночной консолидацией, подмешивается в системный промпт
  working_memory.json     — сериализованный буфер переписки (только сырой транскрипт)
  commitments.md           — список открытых задач/обещаний, общий на личность (не по чату)
  diary/*.md              — факты долгосрочной памяти (по одному файлу на факт)
prompts/                  — служебные системные промпты, не привязанные к личности
  extraction_system.md    — роль модели при фоновом выделении фактов
  extraction_instruction.md — формат ответа при выделении фактов
  consolidation_merge_system.md — роль модели при слиянии похожих фактов
  consolidation_prune_system.md — роль модели при удалении устаревших фактов
  insights_system.md      — роль модели при генерации insights из публичных фактов
  describe_image.md        — роль модели при описании присланных фото
  proactive_nudge.md       — подсказка модели при проактивной рассылке
config.toml               — путь к активной личности, модель, пороги памяти
```

Раз в сутки в 3:00 по локальному времени сервера фоновая задача (`bot-core/src/consolidation.rs` —
расписание, `memory/src/consolidation.rs` — сама логика) сливает похожие факты дневника в один
(силами LLM), удаляет давно не использовавшиеся факты и пересобирает `insights.md` из публичных
фактов — обновление подхватывается ботом сразу, без перезапуска.

С интервалом `[proactive].interval_minutes` (`config.toml`) фоновый воркер (`jobs/proactive.rs`)
выбирает случайный известный чат, в котором давно (`min_inactivity_minutes`) не было сообщений —
живой разговор в выборку не попадает — и с шансом `probability` даёт модели возможность написать
туда первой; решение писать или промолчать (`wait`) остаётся за моделью.

Факты в дневник извлекаются не только по накоплению `token_threshold` токенов в переписке, но и по
простою чата (`idle_extraction.rs`, `memory.idle_extraction_after_minutes`) — иначе для короткой
личной переписки факт мог бы очень долго не появляться в архиве и быть недоступным боту в других
чатах с тем же человеком.

Перед каждым сообщением бот сам (без вызова модели) ищет релевантные факты долгосрочной памяти по
тексту входящего сообщения и подмешивает их в системный промпт (`auto_retrieval_similarity_threshold`/
`auto_retrieval_limit` в `config.toml`) — отдельного инструмента-поиска для этого нет, находка не
зависит от того, догадается ли модель спросить. Заодно тем же вызовом, что извлекает факты,
обновляется отдельный от буфера и от дневника список открытых задач/обещаний личности — общий на
все её чаты, не по одному (`memory/src/commitments.rs`, `personalities/<name>/commitments.md`) —
курируется LLM, не старше нескольких дней.

В `diary/*.md` эмбеддинг хранится одной строкой (числа через запятую), а не YAML-списком —
`serde_yaml` не умеет однострочный (flow-style) вывод, а список на 1000+ чисел делает файл
нечитаемым; чтение старых файлов (ещё YAML-списком) поддерживается для обратной совместимости.

`personalities/` и `.env` — в `.gitignore`, реальные диалоги и токены в репозиторий не попадают.

## Ручной запуск фоновых задач

Не дожидаясь расписания/порогов — например, чтобы проверить, что всё работает:
```
cargo run --bin admin -- extract            # извлечь факты из всех известных чатов в дневник
cargo run --bin admin -- extract --chat-id <id>  # только из одного чата
cargo run --bin admin -- sleep               # прогнать ночную консолидацию прямо сейчас
```
Работает напрямую с файлами буфера/дневника, Telegram не трогает.

## Разработка

```
cargo check
cargo test
cargo clippy --all-targets -- -D warnings
```

