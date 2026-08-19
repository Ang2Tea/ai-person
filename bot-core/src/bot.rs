use std::sync::Arc;

use contracts::{ChannelHistory, ChannelId, ChatMessage, Llm, LlmRole, Memory, Tool, Usage};
use tracing::Instrument;

use crate::{chat_locks::ChatLocks, errors::AppError, tools::ToolRegistry};

const MAX_TOOL_ITERATIONS: usize = 5;
const PROACTIVE_NUDGE_PROMPT: &str = include_str!("../../prompts/proactive_nudge.md");

#[derive(Clone)]
pub struct ChatBot<L, M> {
    history: Arc<dyn ChannelHistory>,
    memory: M,
    llm: L,
    token_threshold: u32,
    tools: Arc<ToolRegistry>,
    chat_locks: ChatLocks,
}

impl<L, M> ChatBot<L, M>
where
    L: Llm + Clone + Send + Sync + 'static,
    M: Memory + Clone + Send + Sync + 'static,
{
    pub fn new(
        history: Arc<dyn ChannelHistory>,
        memory: M,
        llm: L,
        token_threshold: u32,
        tools: Vec<Arc<dyn Tool>>,
    ) -> Self {
        let mut registry = ToolRegistry::new();
        for tool in tools {
            registry.register(tool);
        }

        Self {
            history,
            memory,
            llm,
            token_threshold,
            tools: Arc::new(registry),
            chat_locks: ChatLocks::new(),
        }
    }

    /// Обычный ход — сериализация по чату, tool-calling цикл, фоновое
    /// извлечение памяти. Channel-агностично: не отправляет ответ сам,
    /// только возвращает решённый моделью текст — отправка (и запись
    /// исходящего в историю канала) остаётся заботой вызывающего.
    #[tracing::instrument(skip(self, chat, user, query), fields(chat_id = %chat.id, user_id = %user.id))]
    pub async fn run_turn(
        &self,
        chat: ChannelId,
        user: ChannelId,
        query: &str,
    ) -> Result<Option<String>, AppError> {
        let _chat_guard = self.chat_locks.lock(&chat).await;

        let messages = self.build_messages(&chat, &user, Some(query)).await;
        let (text, last_usage) = self.run_tool_loop(&chat, messages, true).await?;
        self.maybe_spawn_extraction(&chat, last_usage);

        Ok(text)
    }

    /// Проактивный ход — worker периодически выбирает малоактивный чат и даёт
    /// модели шанс написать первой. В отличие от `run_turn`: нет входящего
    /// сообщения (подсказка-нужда добавляется поверх транскрипта, но не
    /// записывается в историю — это не реальное событие), и при исчерпании
    /// итераций без решения ответ не форсируется — промолчать тут нормальный
    /// исход, а не невежливость.
    #[tracing::instrument(skip(self, chat), fields(chat_id = %chat.id))]
    pub async fn run_proactive(&self, chat: ChannelId) -> Result<Option<String>, AppError> {
        let _chat_guard = self.chat_locks.lock(&chat).await;

        let mut messages = self.build_messages(&chat, &chat, None).await;
        messages.push(ChatMessage::user(PROACTIVE_NUDGE_PROMPT.trim()));

        let (text, last_usage) = self.run_tool_loop(&chat, messages, false).await?;
        self.maybe_spawn_extraction(&chat, last_usage);

        Ok(text)
    }

    /// Собирает системный prompt из личности (`Memory::system_prompt`, уже
    /// включает insights), `commitments` этого чата (обновляются вместе с
    /// извлечением фактов) и автоматически найденных релевантных фактов
    /// долгосрочной памяти по тексту входящего сообщения (`query`) — модель
    /// их получает сразу, а не только если сама решит что-то поискать.
    async fn build_messages(
        &self,
        chat: &ChannelId,
        user: &ChannelId,
        query: Option<&str>,
    ) -> Vec<ChatMessage> {
        let mut system_prompt = self.memory.system_prompt().await;

        if let Some(commitments) = self.memory.commitments().await
            && !commitments.is_empty()
        {
            system_prompt.push_str("\n\nОткрытые задачи/обещания:\n");
            system_prompt.push_str(&commitments);
        }

        if let Some(query) = query
            && let Some(relevant) = self
                .memory
                .recall(channel_chat_id(chat), channel_chat_id(user), query)
                .await
        {
            system_prompt.push_str("\n\nИз памяти, возможно относится к происходящему:\n");
            system_prompt.push_str(&relevant);
        }

        let transcript = self.history.transcript(chat).await;
        vec![
            ChatMessage::system(system_prompt),
            ChatMessage::user(transcript),
        ]
    }

    /// Цикл вызовов LLM + диспетчеризация инструментов, общий для обычного
    /// хода и про активного. Возвращает решённый моделью текст (`None`, если
    /// она вызвала инструмент, завершающий ход молча, — или, при
    /// `force_final_answer: false`, просто исчерпала итерации без решения) и
    /// `Usage` последнего вызова (нужна вызывающему для решения о фоновом
    /// извлечении памяти).
    #[tracing::instrument(skip(self, chat, messages), fields(chat_id = %chat.id))]
    async fn run_tool_loop(
        &self,
        chat: &ChannelId,
        mut messages: Vec<ChatMessage>,
        force_final_answer: bool,
    ) -> Result<(Option<String>, Usage), AppError> {
        let mut final_reply: Option<ChatMessage> = None;
        let mut last_usage = Usage::default();

        for i in 0..MAX_TOOL_ITERATIONS {
            let completion = self
                .llm
                .chat(LlmRole::Primary, &messages, &self.tools.specs())
                .await?;
            let reply = completion.message;
            last_usage = completion.usage;
            let calls = reply.tool_calls.clone().unwrap_or_default();

            if calls.is_empty() {
                tracing::debug!(iteration = i, "model requested no tools");
                final_reply = Some(reply);
                break;
            }

            let names: Vec<&str> = calls.iter().map(|c| c.name.as_str()).collect();
            tracing::debug!(iteration = i, tools = ?names, "model requested tool calls");

            messages.push(reply);
            let mut ended_turn = false;
            for call in &calls {
                let result = self.tools.dispatch(call).await;
                messages.push(ChatMessage::tool(call.id.clone(), result));
                ended_turn |= self.tools.ends_turn(&call.name);
            }

            if ended_turn {
                tracing::debug!(iteration = i, "a tool ended the turn silently");
                return Ok((None, last_usage));
            }
        }

        if final_reply.is_none() && force_final_answer {
            tracing::warn!("hit max tool iterations, forcing a final text answer without tools");
            let completion = self.llm.chat(LlmRole::Primary, &messages, &[]).await?;
            last_usage = completion.usage;
            final_reply = Some(completion.message);
        }

        let text = final_reply
            .and_then(|r| r.content)
            .filter(|t| !t.is_empty());

        Ok((text, last_usage))
    }

    fn maybe_spawn_extraction(&self, chat: &ChannelId, last_usage: Usage) {
        if last_usage.total_tokens < self.token_threshold {
            return;
        }

        let history = self.history.clone();
        let memory = self.memory.clone();
        let chat = chat.clone();

        // `tokio::spawn` не наследует текущий span автоматически (задача
        // может быть опрошена на другом потоке) — оборачиваем явно, иначе
        // это фоновое извлечение выпадает из трассировки хода, который его
        // запустил.
        let span = tracing::info_span!("background_extraction", chat_id = %chat.id);
        tokio::spawn(
            async move {
                let transcript = history.transcript(&chat).await;
                memory.extract(channel_chat_id(&chat), &transcript).await;
                history
                    .truncate_keep_last(&chat, memory.keep_last_messages())
                    .await;
            }
            .instrument(span),
        );
    }
}

/// `Memory` в этой фазе рефакторинга всё ещё привязана к числовому Telegram
/// chat/user id (единственный существующий канал), в отличие от уже
/// абстрагированного через `ChannelId` `ChannelHistory` — см. риски плана
/// Фазы 2. `ChannelId::id` для Telegram всегда строка вида `i64.to_string()`
/// (см. `channel_telegram_bot::dispatch`), так что parse здесь не должен
/// падать в реальности.
fn channel_chat_id(id: &ChannelId) -> i64 {
    id.id.parse().unwrap_or(0)
}
