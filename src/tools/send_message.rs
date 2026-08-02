use chrono::Utc;
use serde_json::{Value, json};
use std::pin::Pin;
use teloxide::requests::Requester;
use teloxide::types::ChatId;

use crate::buffer::{BufferStore, BufferedMessage};
use crate::contracts::BufferStorage;
use crate::errors::ToolError;
use crate::tools::Tool;

pub struct SendMessage<B> {
    bot: teloxide::Bot,
    chat_id: ChatId,
    buffer: BufferStore<B>,
}

impl<B> SendMessage<B> {
    pub fn new(bot: teloxide::Bot, chat_id: ChatId, buffer: BufferStore<B>) -> Self {
        Self {
            bot,
            chat_id,
            buffer,
        }
    }
}

impl<B> Tool for SendMessage<B>
where
    B: BufferStorage + Clone + Send + Sync + 'static,
{
    fn name(&self) -> &str {
        "send_message"
    }

    fn spec(&self) -> Value {
        json!({
            "type": "function",
            "function": {
                "name": "send_message",
                "description": "Отправить сообщение пользователю в Telegram.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "text": {
                            "type": "string",
                            "description": "Текст сообщения",
                        },
                    },
                    "required": ["text"],
                },
            },
        })
    }

    fn call<'a>(
        &self,
        args: Value,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>> {
        let bot = self.bot.clone();
        let chat_id = self.chat_id;
        let buffer = self.buffer.clone();

        Box::pin(async move {
            let text = args
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| ToolError::Failed("missing 'text' argument".to_owned()))?
                .to_owned();

            bot.send_message(chat_id, &text)
                .await
                .map_err(|e| ToolError::Failed(e.to_string()))?;

            buffer
                .push(
                    chat_id.0,
                    BufferedMessage {
                        telegram_message_id: 0,
                        sender_id: chat_id.0,
                        sender_name: "bot".to_owned(),
                        text,
                        timestamp: Utc::now(),
                        is_bot: true,
                    },
                )
                .await;

            Ok("message sent".to_owned())
        })
    }
}
