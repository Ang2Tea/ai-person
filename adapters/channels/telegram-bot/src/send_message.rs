use crate::history::{BufferStore, BufferedMessage};
use chrono::Utc;
use contracts::{Storage, Tool, ToolError, ToolSpec};
use serde_json::{Value, json};
use std::future::Future;
use std::pin::Pin;
use teloxide::payloads::SendMessageSetters;
use teloxide::requests::Requester;
use teloxide::types::{ChatId, MessageId, ReplyParameters};

pub struct TelegramSendMessage<B> {
    bot: teloxide::Bot,
    bot_user_id: i64,
    buffer: BufferStore<B>,
}

impl<B> TelegramSendMessage<B> {
    pub fn new(bot: teloxide::Bot, bot_user_id: i64, buffer: BufferStore<B>) -> Self {
        Self {
            bot,
            bot_user_id,
            buffer,
        }
    }
}

impl<B> Tool for TelegramSendMessage<B>
where
    B: Storage + Clone + Send + Sync + 'static,
{
    fn name(&self) -> &str {
        "telegram_send_message"
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "send_message".to_owned(),
            description: "Отправить сообщение в Telegram, в известный чат по его id (список — \
через `list_known_chats`)."
                .to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "Текст сообщения",
                    },
                    "chat_id": {
                        "type": "integer",
                        "description": "id чата, куда отправить сообщение (см. `list_known_chats`). `reply_to_message_id` работает только в пределах того же чата — не указывай их вместе для разных чатов.",
                    },
                    "reply_to_message_id": {
                        "type": "integer",
                        "description": "Необязательно: id сообщения в этом чате, на которое отвечаешь (reply в Telegram). Указывай, только если явно отвечаешь на конкретное сообщение, а не на весь разговор.",
                    },
                },
                "required": ["text", "chat_id"],
            }),
        }
    }

    fn call<'a>(
        &self,
        args: Value,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>> {
        let bot = self.bot.clone();
        let bot_user_id = self.bot_user_id;
        let buffer = self.buffer.clone();

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
                .ok_or_else(|| ToolError::Failed("missing 'chat_id' argument".to_owned()))?;

            let reply_to_message_id = args
                .get("reply_to_message_id")
                .and_then(Value::as_i64)
                .map(|id| MessageId(id as i32));

            #[cfg(feature = "strict-messaging")]
            if crate::text::split_into_paragraphs(&text).len() > 1 {
                return Err(ToolError::Failed(
                    "text содержит пустую строку между абзацами — так нельзя, Telegram отправит \
это одним сообщением. Раздели на несколько отдельных вызовов send_message, по одному сообщению \
на вызов."
                        .to_owned(),
                ));
            }

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
