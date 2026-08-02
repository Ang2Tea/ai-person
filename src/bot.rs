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
    contracts::{BufferStorage, ChatMessage},
    errors::AppError,
    tools::{GetCurrentDatetime, SendMessage, ToolContext, ToolRegistry, Wait},
};

const MODEL: &str = "deepseek/deepseek-v4-flash";
const MAX_TOOL_ITERATIONS: usize = 5;

#[derive(Clone)]
pub struct ChatBot<B> {
    bot: Bot,
    llm: TimewebClient,
    buffer: BufferStore<B>,
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
        system_prompt: impl Into<Arc<str>>,
    ) -> Self {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(GetCurrentDatetime));
        registry.register(Arc::new(Wait));
        registry.register(Arc::new(SendMessage::new(bot.clone())));

        Self {
            bot,
            llm,
            buffer,
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
            buffer: self.buffer.clone(),
        };

        let mut final_reply: Option<ChatMessage> = None;

        for i in 0..MAX_TOOL_ITERATIONS {
            let reply = self
                .llm
                .chat(MODEL, &messages, &self.tools.specs())
                .await?;
            let calls = reply.tool_calls.clone().unwrap_or_default();

            if let Some(wait_call) = calls.iter().find(|c| c.function.name == "wait") {
                tracing::debug!(chat_id = chat_id.0, "model chose to wait, ending turn silently");
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

        let Some(text) = final_reply.and_then(|r| r.content).filter(|t| !t.is_empty()) else {
            return Ok(());
        };

        self.bot.send_message(msg.chat.id, &text).await?;

        let outgoing = BufferedMessage {
            telegram_message_id: msg.id.0,
            sender_id: from.id.0 as i64,
            sender_name: "bot".to_owned(),
            text,
            timestamp: Utc::now(),
            is_bot: true,
        };

        self.buffer.push(chat_id.0, outgoing).await;

        Ok(())
    }
}
