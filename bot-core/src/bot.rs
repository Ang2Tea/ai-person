use std::cmp::Ordering;
use std::sync::Arc;

use chrono::Utc;
use teloxide::{
    Bot,
    dispatching::dialogue::GetChatId,
    requests::Requester,
    types::{ChatId, MaybeAnonymousUser, Message, MessageReactionUpdated, ReactionType},
};

use contracts::{ChatMessage, Storage, Usage};
use llm_timeweb::TimewebClient;

use crate::{
    buffer::{BufferStore, BufferedMessage, ChatBuffer},
    chat_locks::ChatLocks,
    commitments::CommitmentsStore,
    consolidation::SharedInsights,
    errors::AppError,
    memory::{self, MemoryRecord, MemoryStore, Visibility},
    settings::MemorySettings,
    tools::{
        GetCurrentDatetime, ListKnownChats, ReadChatHistory, Remember, SendMessage, SendReaction,
        ToolContext, ToolRegistry, Wait,
    },
};

const MAX_TOOL_ITERATIONS: usize = 5;
const PROACTIVE_NUDGE_PROMPT: &str = include_str!("../../prompts/proactive_nudge.md");

#[derive(Clone)]
pub struct ChatBot<B> {
    bot: Bot,
    bot_user_id: i64,
    llm: TimewebClient,
    buffer: BufferStore<B>,
    memory: MemoryStore,
    memory_settings: MemorySettings,
    commitments: CommitmentsStore,
    model: Arc<str>,
    embedding_model: Arc<str>,
    system_prompt: Arc<str>,
    insights: SharedInsights,
    tools: Arc<ToolRegistry<B>>,
    chat_locks: ChatLocks,
}

impl<B> ChatBot<B>
where
    B: Storage + Clone + Send + Sync + 'static,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        bot: Bot,
        bot_user_id: i64,
        llm: TimewebClient,
        buffer: BufferStore<B>,
        memory: MemoryStore,
        memory_settings: MemorySettings,
        commitments: CommitmentsStore,
        model: impl Into<Arc<str>>,
        embedding_model: impl Into<Arc<str>>,
        system_prompt: impl Into<Arc<str>>,
        insights: SharedInsights,
    ) -> Self {
        let embedding_model: Arc<str> = embedding_model.into();

        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(GetCurrentDatetime));
        registry.register(Arc::new(Wait));
        registry.register(Arc::new(SendMessage::new(bot.clone(), bot_user_id)));
        registry.register(Arc::new(SendReaction::new(bot.clone())));
        registry.register(Arc::new(ListKnownChats::new(buffer.clone())));
        registry.register(Arc::new(ReadChatHistory::new(buffer.clone())));
        registry.register(Arc::new(Remember::new(
            llm.clone(),
            memory.clone(),
            memory_settings.clone(),
            embedding_model.clone(),
        )));

        Self {
            bot,
            bot_user_id,
            llm,
            buffer,
            memory,
            memory_settings,
            commitments,
            model: model.into(),
            embedding_model,
            system_prompt: system_prompt.into(),
            insights,
            tools: Arc::new(registry),
            chat_locks: ChatLocks::new(),
        }
    }

    pub async fn handle_message(&self, msg: Message) -> Result<(), AppError> {
        let Some(chat_id) = msg.chat_id() else {
            tracing::error!("Can`t get chat id");
            return Ok(());
        };

        let Some(text) = describe_message(&msg) else {
            tracing::debug!("Skipping message without representable content");
            return Ok(());
        };

        let Some(from) = &msg.from else {
            tracing::error!("Can`t get message sender");
            return Ok(());
        };

        let incoming = BufferedMessage {
            telegram_message_id: msg.id.0,
            sender_id: ChatId::from(from.id).0,
            sender_name: from.first_name.clone(),
            text,
            timestamp: Utc::now(),
            is_bot: false,
        };

        self.run_turn(chat_id, from.id.0 as i64, incoming).await
    }

    /// Правки в Telegram применяются только к тексту/подписи — дайс, стикер,
    /// опрос и т.п. отредактировать в другой тип контента нельзя, поэтому
    /// здесь достаточно текста/подписи, в отличие от `describe_message`.
    pub async fn handle_edited_message(&self, msg: Message) -> Result<(), AppError> {
        let Some(chat_id) = msg.chat_id() else {
            tracing::error!("Can`t get chat id for edited message");
            return Ok(());
        };

        let Some(new_text) = msg.text().or_else(|| msg.caption()) else {
            tracing::debug!("Skipping edited message without text/caption");
            return Ok(());
        };

        let Some(from) = &msg.from else {
            tracing::error!("Can`t get edited message sender");
            return Ok(());
        };

        let incoming = BufferedMessage {
            telegram_message_id: msg.id.0,
            sender_id: ChatId::from(from.id).0,
            sender_name: from.first_name.clone(),
            text: format!(
                "{} отредактировал(а) сообщение #{}, теперь: {}",
                from.first_name, msg.id.0, new_text
            ),
            timestamp: Utc::now(),
            is_bot: false,
        };

        self.run_turn(chat_id, from.id.0 as i64, incoming).await
    }

    pub async fn handle_reaction(&self, reaction: MessageReactionUpdated) -> Result<(), AppError> {
        let MaybeAnonymousUser::User(user) = &reaction.actor else {
            tracing::debug!("Skipping reaction from an anonymous/channel actor");
            return Ok(());
        };

        if user.id.0 as i64 == self.bot_user_id {
            // Не реагируем на собственные же реакции — иначе потенциальный цикл.
            return Ok(());
        }

        if reaction.new_reaction.is_empty() {
            // Реакцию сняли, а не поставили — этот случай не описан заданием.
            return Ok(());
        }

        let chat_id = reaction.chat.id;
        let emoji = describe_reactions(&reaction.new_reaction);

        let incoming = BufferedMessage {
            telegram_message_id: reaction.message_id.0,
            sender_id: user.id.0 as i64,
            sender_name: user.first_name.clone(),
            text: format!(
                "{} поставил(а) реакцию {} на сообщение #{}",
                user.first_name, emoji, reaction.message_id.0
            ),
            timestamp: reaction.date,
            is_bot: false,
        };

        self.run_turn(chat_id, user.id.0 as i64, incoming).await
    }

    /// Общий путь для обычного сообщения, правки и реакции — сериализация по
    /// чату, буфер, tool-calling цикл, отправка ответа, фоновое извлечение
    /// памяти. Захват `chat_locks` здесь, а не в каждом из входов, потому что
    /// сериализация нужна вокруг push/LLM-вызова/ответа, а не вокруг разбора
    /// конкретного вида апдейта.
    async fn run_turn(
        &self,
        chat_id: ChatId,
        user_id: i64,
        incoming: BufferedMessage,
    ) -> Result<(), AppError> {
        // Держим до конца функции (обычный drop в конце скоупа); фоновое
        // извлечение памяти ниже этим локом не оборачивается — оно не должно
        // блокировать обработку следующего апдейта.
        let _chat_guard = self.chat_locks.lock(chat_id.0).await;

        let query = incoming.text.clone();
        self.buffer.push(chat_id.0, incoming).await;

        let Some(chat_buffer) = self.buffer.get(chat_id.0).await else {
            tracing::error!("Can`t get chat buffer");
            return Ok(());
        };

        let messages = self
            .build_messages(&chat_buffer, chat_id, user_id, Some(&query))
            .await;
        let ctx = ToolContext {
            chat_id,
            user_id,
            buffer: self.buffer.clone(),
        };

        let (text, last_usage) = self.run_tool_loop(chat_id, &ctx, messages, true).await?;
        let Some(text) = text else {
            return Ok(());
        };

        self.send_and_record(chat_id, text).await?;
        self.maybe_spawn_extraction(chat_id, last_usage);

        Ok(())
    }

    /// Проактивный ход — воркер (`proactive.rs`) периодически выбирает
    /// малоактивный чат и даёт модели шанс написать первой. В отличие от
    /// `run_turn`: нет входящего сообщения (подсказка-нудж добавляется поверх
    /// транскрипта, но в буфер не кладётся — это не реальное событие), и при
    /// исчерпании итераций без решения ответ не форсируется — промолчать тут
    /// нормальный исход, а не невежливость.
    pub async fn run_proactive(&self, chat_id: ChatId) -> Result<(), AppError> {
        let _chat_guard = self.chat_locks.lock(chat_id.0).await;

        let Some(chat_buffer) = self.buffer.get(chat_id.0).await else {
            tracing::debug!(chat_id = chat_id.0, "proactive: chat not known, skipping");
            return Ok(());
        };

        let mut messages = self
            .build_messages(&chat_buffer, chat_id, self.bot_user_id, None)
            .await;
        messages.push(ChatMessage::user(PROACTIVE_NUDGE_PROMPT.trim()));

        let ctx = ToolContext {
            chat_id,
            user_id: self.bot_user_id,
            buffer: self.buffer.clone(),
        };

        let (text, last_usage) = self.run_tool_loop(chat_id, &ctx, messages, false).await?;
        let Some(text) = text else {
            tracing::debug!(chat_id = chat_id.0, "proactive: model chose not to write");
            return Ok(());
        };

        self.send_and_record(chat_id, text).await?;
        self.maybe_spawn_extraction(chat_id, last_usage);

        Ok(())
    }

    /// Собирает системный промпт из статической личности, `insights`
    /// (обновляется ночной консолидацией), `commitments` этого чата
    /// (обновляется вместе с извлечением фактов, см. `memory::maybe_extract`)
    /// и автоматически найденных релевантных фактов долгосрочной памяти по
    /// тексту входящего сообщения (`query`) — модель их получает сразу, а не
    /// только если сама решит что-то поискать.
    async fn build_messages(
        &self,
        chat_buffer: &ChatBuffer,
        chat_id: ChatId,
        user_id: i64,
        query: Option<&str>,
    ) -> Vec<ChatMessage> {
        let mut system_prompt = self.system_prompt.to_string();

        let insights = self.insights.read().await.clone();
        if !insights.is_empty() {
            system_prompt.push_str("\n\n");
            system_prompt.push_str(&insights);
        }

        if let Some(commitments) = self.commitments.get(chat_id.0).await
            && !commitments.is_empty()
        {
            system_prompt.push_str("\n\nОткрытые задачи/обещания в этом чате:\n");
            system_prompt.push_str(&commitments);
        }

        if let Some(query) = query
            && let Some(relevant) = self.retrieve_relevant_facts(chat_id, user_id, query).await
        {
            system_prompt.push_str("\n\nИз памяти, возможно относится к происходящему:\n");
            system_prompt.push_str(&relevant);
        }

        chat_buffer.to_request_messages(&system_prompt)
    }

    /// Автоматический поиск по долгосрочной памяти вместо инструмента, который
    /// модель должна была бы сама решить вызвать — иначе она не всегда
    /// догадывается спросить, и бот "не помнит" собеседника в другом чате.
    /// Пороги строже, чем были бы у ручного инструмента: срабатывает на
    /// каждое сообщение, значит должен быть придирчивее, чтобы не забивать
    /// контекст маловероятным. Любая ошибка — тихо `None`, не роняя ход.
    async fn retrieve_relevant_facts(
        &self,
        chat_id: ChatId,
        user_id: i64,
        query: &str,
    ) -> Option<String> {
        let query_embedding = self
            .llm
            .embed(&self.embedding_model, query)
            .await
            .inspect_err(|err| tracing::debug!(%err, "auto-retrieval: embedding failed"))
            .ok()?;

        let own_prefix = format!("{}--", chat_id.0);
        let about_me_token = format!(",{user_id},");
        let records = self
            .memory
            .list_filtered(move |name| {
                name.starts_with(&own_prefix)
                    || name.contains("--public--")
                    || name.contains(&about_me_token)
            })
            .await
            .inspect_err(|err| tracing::debug!(%err, "auto-retrieval: listing records failed"))
            .ok()?;

        let mut matches: Vec<(f32, MemoryRecord)> = Vec::new();
        for record in records {
            if !is_visible(&record, chat_id.0, user_id) {
                continue;
            }

            let score = memory::cosine_similarity(&record.embedding, &query_embedding);
            if score >= self.memory_settings.auto_retrieval_similarity_threshold {
                matches.push((score, record));
            }
        }

        if matches.is_empty() {
            return None;
        }

        matches.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal));
        matches.truncate(self.memory_settings.auto_retrieval_limit);

        let mut result = String::new();
        for (_, mut record) in matches {
            result.push_str(&format!(
                "- [уверенность: {}] {}\n",
                record.confidence,
                record.text.trim()
            ));

            record.usage_count += 1;
            record.last_used = Some(Utc::now());
            if let Err(err) = self.memory.touch(&record).await {
                tracing::warn!(%err, "auto-retrieval: failed to update memory record usage stats");
            }
        }

        Some(result.trim().to_owned())
    }

    /// Цикл вызовов LLM + диспетчеризация инструментов, общий для обычного
    /// хода и проактивного. Возвращает решённый моделью текст (`None`, если
    /// она вызвала `wait` или — при `force_final_answer: false` — просто
    /// исчерпала итерации без решения) и `Usage` последнего вызова (нужна
    /// вызывающему для решения о фоновом извлечении памяти).
    async fn run_tool_loop(
        &self,
        chat_id: ChatId,
        ctx: &ToolContext<B>,
        mut messages: Vec<ChatMessage>,
        force_final_answer: bool,
    ) -> Result<(Option<String>, Usage), AppError> {
        let mut final_reply: Option<ChatMessage> = None;
        let mut last_usage = Usage::default();

        for i in 0..MAX_TOOL_ITERATIONS {
            let completion = self
                .llm
                .chat(&self.model, &messages, &self.tools.specs())
                .await?;
            let reply = completion.message;
            last_usage = completion.usage;
            let calls = reply.tool_calls.clone().unwrap_or_default();

            if calls.is_empty() {
                tracing::debug!(chat_id = chat_id.0, iteration = i, "model requested no tools");
            } else {
                let names: Vec<&str> = calls.iter().map(|c| c.function.name.as_str()).collect();
                tracing::debug!(chat_id = chat_id.0, iteration = i, tools = ?names, "model requested tool calls");
            }

            if let Some(wait_call) = calls.iter().find(|c| c.function.name == "wait") {
                tracing::debug!(
                    chat_id = chat_id.0,
                    "model chose to wait, ending turn silently"
                );
                self.tools.dispatch(wait_call, ctx).await;
                return Ok((None, last_usage));
            }

            if calls.is_empty() {
                final_reply = Some(reply);
                break;
            }

            messages.push(reply);
            for call in &calls {
                let result = self.tools.dispatch(call, ctx).await;
                messages.push(ChatMessage::tool(call.id.clone(), result));
            }
        }

        if final_reply.is_none() && force_final_answer {
            tracing::warn!(
                chat_id = chat_id.0,
                "hit max tool iterations, forcing a final text answer without tools"
            );
            let completion = self.llm.chat(&self.model, &messages, &[]).await?;
            last_usage = completion.usage;
            final_reply = Some(completion.message);
        }

        let text = final_reply
            .and_then(|r| r.content)
            .filter(|t| !t.is_empty());

        Ok((text, last_usage))
    }

    async fn send_and_record(&self, chat_id: ChatId, text: String) -> Result<(), AppError> {
        let sent = self.bot.send_message(chat_id, &text).await?;

        let outgoing = BufferedMessage {
            telegram_message_id: sent.id.0,
            sender_id: self.bot_user_id,
            sender_name: "bot".to_owned(),
            text,
            timestamp: Utc::now(),
            is_bot: true,
        };

        self.buffer.push(chat_id.0, outgoing).await;

        Ok(())
    }

    fn maybe_spawn_extraction(&self, chat_id: ChatId, last_usage: Usage) {
        if last_usage.total_tokens < self.memory_settings.token_threshold {
            return;
        }

        let llm = self.llm.clone();
        let memory = self.memory.clone();
        let buffer = self.buffer.clone();
        let commitments = self.commitments.clone();
        let settings = self.memory_settings.clone();
        let model = self.model.clone();
        let embedding_model = self.embedding_model.clone();
        let raw_chat_id = chat_id.0;

        tokio::spawn(async move {
            if let Err(err) = memory::maybe_extract(
                &llm,
                &memory,
                &buffer,
                &commitments,
                raw_chat_id,
                &settings,
                &model,
                &embedding_model,
            )
            .await
            {
                tracing::error!(chat_id = raw_chat_id, %err, "memory extraction failed");
            }
        });
    }
}

/// Текстовое описание содержимого сообщения — вместо голого `.text()`, чтобы
/// опросы/дайсы/геолокация/контакты/стикеры/подписи тоже попадали в буфер и
/// проходили через общий tool-calling цикл, а не отбрасывались молча. Сама
/// медиа-часть (фото/стикер/голос) не разбирается — это отдельная, более
/// дорогая задача (нужен доп. вызов другой модели).
fn describe_message(msg: &Message) -> Option<String> {
    if let Some(text) = msg.text() {
        return Some(text.to_owned());
    }
    if let Some(caption) = msg.caption() {
        return Some(caption.to_owned());
    }
    if let Some(poll) = msg.poll() {
        let options = poll
            .options
            .iter()
            .map(|o| o.text.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let mut description = format!("запустил опрос: {} — варианты: {}", poll.question, options);
        if let Some(explanation) = &poll.explanation {
            description.push_str(&format!(" (пояснение: {explanation})"));
        }
        return Some(description);
    }
    if let Some(dice) = msg.dice() {
        let emoji = match dice.emoji {
            teloxide::types::DiceEmoji::Dice => "🎲",
            teloxide::types::DiceEmoji::Darts => "🎯",
            teloxide::types::DiceEmoji::Bowling => "🎳",
            teloxide::types::DiceEmoji::Basketball => "🏀",
            teloxide::types::DiceEmoji::Football => "⚽",
            teloxide::types::DiceEmoji::SlotMachine => "🎰",
        };
        return Some(format!("бросил {emoji}, результат: {}", dice.value));
    }
    if let Some(venue) = msg.venue() {
        return Some(format!(
            "поделился геолокацией: {} ({})",
            venue.title, venue.address
        ));
    }
    if msg.location().is_some() {
        return Some("поделился геолокацией".to_owned());
    }
    if let Some(contact) = msg.contact() {
        let last_name = contact.last_name.clone().unwrap_or_default();
        return Some(
            format!(
                "поделился контактом: {} {}, {}",
                contact.first_name, last_name, contact.phone_number
            )
            .trim()
            .to_owned(),
        );
    }
    if let Some(sticker) = msg.sticker() {
        let emoji = sticker.emoji.clone().unwrap_or_default();
        return Some(format!("отправил стикер {emoji}"));
    }

    None
}

/// Запись видна из чата `chat_id` от лица пользователя `user_id`, если она
/// публичная, либо возникла в этом же чате, либо лично про этого пользователя
/// (даже если приватная и из другого чата).
fn is_visible(record: &MemoryRecord, chat_id: i64, user_id: i64) -> bool {
    record.visibility == Visibility::Public
        || record.origin_chat_id == chat_id
        || record.about_users.contains(&user_id)
}

/// Эмодзи, реально поставленные (после изменения) — для текста в буфер.
fn describe_reactions(reactions: &[ReactionType]) -> String {
    reactions
        .iter()
        .map(|r| match r {
            ReactionType::Emoji { emoji } => emoji.clone(),
            ReactionType::CustomEmoji { .. } => "кастомный эмодзи".to_owned(),
            ReactionType::Paid => "⭐".to_owned(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}
