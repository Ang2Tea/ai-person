use std::sync::Arc;

use chrono::Utc;
use teloxide::{
    Bot,
    dispatching::dialogue::GetChatId,
    requests::Requester,
    types::{ChatId, Message},
};

use crate::{
    adapters::timeweb_client::TimewebClient,
    buffer::{BufferStore, BufferedMessage},
    contracts::{BufferStorage, ChatMessage, Usage},
    errors::AppError,
    memory::{self, MemoryStore},
    settings::MemorySettings,
    tools::{
        GetCurrentDatetime, ListKnownChats, ReadChatHistory, Remember, SearchMemory, SendMessage,
        ToolContext, ToolRegistry, Wait,
    },
};

const MODEL: &str = "deepseek/deepseek-v4-flash";
const MAX_TOOL_ITERATIONS: usize = 5;

#[derive(Clone)]
pub struct ChatBot<B> {
    bot: Bot,
    llm: TimewebClient,
    buffer: BufferStore<B>,
    memory: MemoryStore,
    memory_settings: MemorySettings,
    system_prompt: Arc<str>,
    tools: Arc<ToolRegistry<B>>,
}

impl<B> ChatBot<B>
where
    B: BufferStorage + Clone + Send + Sync + 'static,
{
    pub fn new(
        bot: Bot,
        llm: TimewebClient,
        buffer: BufferStore<B>,
        memory: MemoryStore,
        memory_settings: MemorySettings,
        system_prompt: impl Into<Arc<str>>,
    ) -> Self {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(GetCurrentDatetime));
        registry.register(Arc::new(Wait));
        registry.register(Arc::new(SendMessage::new(bot.clone())));
        registry.register(Arc::new(ListKnownChats::new(buffer.clone())));
        registry.register(Arc::new(ReadChatHistory::new(buffer.clone())));
        registry.register(Arc::new(SearchMemory::new(
            llm.clone(),
            memory.clone(),
            memory_settings.clone(),
        )));
        registry.register(Arc::new(Remember::new(
            llm.clone(),
            memory.clone(),
            memory_settings.clone(),
        )));

        Self {
            bot,
            llm,
            buffer,
            memory,
            memory_settings,
            system_prompt: system_prompt.into(),
            tools: Arc::new(registry),
        }
    }

    pub async fn handle_message(&self, msg: Message) -> Result<(), AppError> {
        let Some(chat_id) = msg.chat_id() else {
            tracing::error!("Can`t get chat id");
            return Ok(());
        };

        let Some(text) = msg.text() else {
            tracing::debug!("Skipping non-text message");
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
            text: text.to_owned(),
            timestamp: Utc::now(),
            is_bot: false,
        };

        self.buffer.push(chat_id.0, incoming).await;

        let Some(chat_buffer) = self.buffer.get(chat_id.0).await else {
            tracing::error!("Can`t get chat buffer");
            return Ok(());
        };

        let mut messages = chat_buffer.to_request_messages(&self.system_prompt);

        let ctx = ToolContext {
            chat_id,
            user_id: from.id.0 as i64,
            buffer: self.buffer.clone(),
        };

        let mut final_reply: Option<ChatMessage> = None;
        let mut last_usage = Usage::default();

        for i in 0..MAX_TOOL_ITERATIONS {
            let completion = self.llm.chat(MODEL, &messages, &self.tools.specs()).await?;
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
                self.tools.dispatch(wait_call, &ctx).await;
                return Ok(());
            }

            if calls.is_empty() {
                final_reply = Some(reply);
                break;
            }

            if i == MAX_TOOL_ITERATIONS - 1 {
                tracing::warn!(
                    chat_id = chat_id.0,
                    "hit max tool iterations, using last reply as-is"
                );
                final_reply = Some(reply);
                break;
            }

            messages.push(reply);
            for call in &calls {
                let result = self.tools.dispatch(call, &ctx).await;
                messages.push(ChatMessage::tool_result(call.id.clone(), result));
            }
        }

        let Some(text) = final_reply
            .and_then(|r| r.content)
            .filter(|t| !t.is_empty())
        else {
            return Ok(());
        };

        let sent = self.bot.send_message(msg.chat.id, &text).await?;

        let outgoing = BufferedMessage {
            telegram_message_id: sent.id.0,
            sender_id: from.id.0 as i64,
            sender_name: "bot".to_owned(),
            text,
            timestamp: Utc::now(),
            is_bot: true,
        };

        self.buffer.push(chat_id.0, outgoing).await;

        if last_usage.total_tokens >= self.memory_settings.token_threshold {
            let llm = self.llm.clone();
            let memory = self.memory.clone();
            let buffer = self.buffer.clone();
            let settings = self.memory_settings.clone();
            let raw_chat_id = chat_id.0;

            tokio::spawn(async move {
                if let Err(err) =
                    memory::maybe_extract(&llm, &memory, &buffer, raw_chat_id, &settings).await
                {
                    tracing::error!(chat_id = raw_chat_id, %err, "memory extraction failed");
                }
            });
        }

        Ok(())
    }
}
