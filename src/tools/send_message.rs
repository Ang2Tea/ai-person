use chrono::Utc;
use serde_json::{Value, json};
use std::pin::Pin;
use teloxide::requests::Requester;

use crate::buffer::BufferedMessage;
use crate::contracts::BufferStorage;
use crate::errors::ToolError;
use crate::tools::{Tool, ToolContext};

pub struct SendMessage {
    bot: teloxide::Bot,
}

impl SendMessage {
    pub fn new(bot: teloxide::Bot) -> Self {
        Self { bot }
    }
}

impl<B> Tool<B> for SendMessage
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
        ctx: &ToolContext<B>,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>> {
        let bot = self.bot.clone();
        let chat_id = ctx.chat_id;
        let buffer = ctx.buffer.clone();

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
