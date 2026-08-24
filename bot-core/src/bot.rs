use std::sync::Arc;

use contracts::{ChannelHistory, ChannelId, ChatMessage, Llm, LlmRole, Memory, Tool, Usage};
use tracing::Instrument;

use crate::{chat_locks::ChatLocks, errors::AppError, tools::ToolRegistry};

const MAX_TOOL_ITERATIONS: usize = 5;
const PROACTIVE_NUDGE_PROMPT: &str = include_str!("../../prompts/proactive_nudge.md");
const DESCRIBE_IMAGE_PROMPT: &str = include_str!("../../prompts/describe_image.md");

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
    /// извлечение памяти. Channel-агностично и ничего не отправляет само по
    /// возвращаемому значению: единственный способ доставить что-то
    /// собеседнику — модель сама вызывает инструмент отправки (например,
    /// `send_message`), который шлёт сообщение и пишет его в историю канала
    /// как свой побочный эффект. Если модель за ход не вызвала ни одного
    /// такого инструмента — ход просто завершается молча, это нормальный
    /// исход, а не ошибка.
    #[tracing::instrument(skip(self, chat, user, query), fields(chat_id = %chat.id, user_id = %user.id))]
    pub async fn run_turn(&self, chat: ChannelId, user: ChannelId, query: &str) -> Result<(), AppError> {
        let _chat_guard = self.chat_locks.lock(&chat).await;

        let messages = self.build_messages(&chat, &user, Some(query)).await;
        let last_usage = self.run_tool_loop(&chat, messages).await?;
        self.maybe_spawn_extraction(&chat, last_usage);

        Ok(())
    }

    /// Проактивный ход — worker периодически выбирает малоактивный чат и даёт
    /// модели шанс написать первой. В отличие от `run_turn`: нет входящего
    /// сообщения (подсказка-нужда добавляется поверх транскрипта, но не
    /// записывается в историю — это не реальное событие). Как и в `run_turn`,
    /// промолчать (не вызвать инструмент отправки) — нормальный исход.
    #[tracing::instrument(skip(self, chat), fields(chat_id = %chat.id))]
    pub async fn run_proactive(&self, chat: ChannelId) -> Result<(), AppError> {
        let _chat_guard = self.chat_locks.lock(&chat).await;

        let mut messages = self.build_messages(&chat, &chat, None).await;
        messages.push(ChatMessage::user(PROACTIVE_NUDGE_PROMPT.trim()));

        let last_usage = self.run_tool_loop(&chat, messages).await?;
        self.maybe_spawn_extraction(&chat, last_usage);

        Ok(())
    }

    /// Разовое описание статичного изображения текстом — вне tool-calling
    /// цикла и вне истории чата. Вызывающий (канал) сам скачивает байты и
    /// подставляет получившийся текст в свой обычный текстовый пайплайн, как
    /// если бы это было обычное сообщение — `ChatBot` про Telegram/файлы
    /// ничего не знает.
    pub async fn describe_image(
        &self,
        image_bytes: &[u8],
        mime_type: &str,
    ) -> Result<String, AppError> {
        self.llm
            .describe_image(LlmRole::Vision, image_bytes, mime_type, DESCRIBE_IMAGE_PROMPT)
            .await
            .map_err(AppError::from)
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
    /// хода и проактивного. Ничего не возвращает, кроме `Usage` последнего
    /// вызова (нужна вызывающему для решения о фоновом извлечении памяти) —
    /// сама доставка сообщения (если она вообще случилась в этот ход) уже
    /// произошла как побочный эффект вызова инструмента отправки внутри
    /// `self.tools.dispatch`. Обычный текстовый ответ модели без вызовов
    /// инструментов никуда не отправляется и просто завершает ход — как и
    /// исчерпание `MAX_TOOL_ITERATIONS` без единого вызова: это осознанно не
    /// форсируется, промолчать — нормальный исход.
    #[tracing::instrument(skip(self, chat, messages), fields(chat_id = %chat.id))]
    async fn run_tool_loop(&self, chat: &ChannelId, mut messages: Vec<ChatMessage>) -> Result<Usage, AppError> {
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
                tracing::debug!(iteration = i, "model ended the turn without sending anything");
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
                break;
            }
        }

        Ok(last_usage)
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
