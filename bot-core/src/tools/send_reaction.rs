use serde_json::{Value, json};
use std::pin::Pin;
use teloxide::payloads::SetMessageReactionSetters;
use teloxide::requests::Requester;
use teloxide::types::{ChatId, MessageId, ReactionType};

use crate::errors::ToolError;
use crate::tools::{Tool, ToolContext, is_chat_access_allowed};

pub struct SendReaction {
    bot: teloxide::Bot,
}

impl SendReaction {
    pub fn new(bot: teloxide::Bot) -> Self {
        Self { bot }
    }
}

impl<B: Send + Sync + 'static> Tool<B> for SendReaction {
    fn name(&self) -> &str {
        "send_reaction"
    }

    fn spec(&self) -> Value {
        json!({
            "type": "function",
            "function": {
                "name": "send_reaction",
                "description": "Поставить эмодзи-реакцию на сообщение по его id (виден в транскрипте \
        как #id). Не любой эмодзи разрешён Telegram — если сервер отклонит конкретный эмодзи, \
        вернётся текст ошибки, а не сработает молча.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "message_id": {
                            "type": "integer",
                            "description": "id сообщения из транскрипта (#id), на которое ставится реакция",
                        },
                        "emoji": {
                            "type": "string",
                            "description": "Эмодзи реакции, например 👍",
                        },
                        "chat_id": {
                            "type": "integer",
                            "description": "Необязательно: id чата, если не текущий (см. `list_known_chats`).",
                        },
                    },
                    "required": ["message_id", "emoji"],
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
        let current_chat_id = ctx.chat_id;

        Box::pin(async move {
            let message_id = args
                .get("message_id")
                .and_then(Value::as_i64)
                .ok_or_else(|| ToolError::Failed("missing 'message_id' argument".to_owned()))?
                as i32;
            let emoji = args
                .get("emoji")
                .and_then(Value::as_str)
                .ok_or_else(|| ToolError::Failed("missing 'emoji' argument".to_owned()))?
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
                    "model tried to send_reaction into a chat it isn't allowed to reach"
                );
                return Ok("нет доступа для реакции в этом чате".to_owned());
            }

            bot.set_message_reaction(chat_id, MessageId(message_id))
                .reaction(vec![ReactionType::Emoji { emoji }])
                .await
                .map_err(|e| ToolError::Failed(e.to_string()))?;

            Ok("реакция поставлена".to_owned())
        })
    }
}
