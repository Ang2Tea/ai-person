use chrono::Utc;
use serde_json::{Value, json};
use std::pin::Pin;
use teloxide::payloads::SendMessageSetters;
use teloxide::requests::Requester;
use teloxide::types::{ChatId, MessageId, ReplyParameters};

use crate::buffer::BufferedMessage;
use crate::contracts::BufferStorage;
use crate::errors::ToolError;
use crate::tools::{Tool, ToolContext, is_chat_access_allowed};

pub struct SendMessage {
    bot: teloxide::Bot,
    bot_user_id: i64,
}

impl SendMessage {
    pub fn new(bot: teloxide::Bot, bot_user_id: i64) -> Self {
        Self { bot, bot_user_id }
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
                "description": "Отправить сообщение в Telegram. По умолчанию — в текущий чат, \
        но можно явно указать `chat_id`, чтобы отправить в другой известный чат (список — через \
        `list_known_chats`).",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "text": {
                            "type": "string",
                            "description": "Текст сообщения",
                        },
                        "chat_id": {
                            "type": "integer",
                            "description": "Необязательно: id чата, куда отправить сообщение, если не в текущий (см. `list_known_chats`). `reply_to_message_id` работает только в пределах текущего чата — не указывай их вместе.",
                        },
                        "reply_to_message_id": {
                            "type": "integer",
                            "description": "Необязательно: id сообщения в текущем чате, на которое отвечаешь (реплай в Telegram). Указывай, только если явно отвечаешь на конкретное сообщение, а не на весь разговор.",
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
        let bot_user_id = self.bot_user_id;
        let current_chat_id = ctx.chat_id;
        let buffer = ctx.buffer.clone();

        Box::pin(async move {
            let text = args
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| ToolError::Failed("missing 'text' argument".to_owned()))?
                .to_owned();
            let chat_id = args
                .get("chat_id")
                .and_then(Value::as_i64)
                .map(ChatId)
                .unwrap_or(current_chat_id);

            if !is_chat_access_allowed(current_chat_id.0, chat_id.0) {
                tracing::warn!(
                    current_chat_id = current_chat_id.0,
                    requested_chat_id = chat_id.0,
                    "model tried to send_message into a chat it isn't allowed to reach"
                );
                return Ok("нет доступа для отправки в этот чат".to_owned());
            }

            let reply_to_message_id = args
                .get("reply_to_message_id")
                .and_then(Value::as_i64)
                .map(|id| MessageId(id as i32));

            let mut request = bot.send_message(chat_id, &text);
            if let Some(message_id) = reply_to_message_id {
                request = request.reply_parameters(ReplyParameters::new(message_id));
            }
            let sent = request
                .await
                .map_err(|e| ToolError::Failed(e.to_string()))?;

            buffer
                .push(
                    chat_id.0,
                    BufferedMessage {
                        telegram_message_id: sent.id.0,
                        sender_id: bot_user_id,
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
