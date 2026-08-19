use contracts::{Tool, ToolError, ToolSpec};
use serde_json::{Value, json};
use std::future::Future;
use std::pin::Pin;
use teloxide::payloads::SetMessageReactionSetters;
use teloxide::requests::Requester;
use teloxide::types::{ChatId, MessageId, ReactionType};

pub struct TelegramSendReaction {
    bot: teloxide::Bot,
}

impl TelegramSendReaction {
    pub fn new(bot: teloxide::Bot) -> Self {
        Self { bot }
    }
}

impl Tool for TelegramSendReaction {
    fn name(&self) -> &str {
        "telegram_send_reaction"
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "send_reaction".to_owned(),
            description: "Поставить эмодзи-реакцию на сообщение по его id (виден в транскрипте \
как #id) в известном чате. Не любой эмодзи разрешён Telegram — если сервер отклонит конкретный \
эмодзи, вернётся текст ошибки, а не сработает молча."
                .to_owned(),
            parameters: json!({
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
                        "description": "id чата, в котором находится сообщение (см. `list_known_chats`).",
                    },
                },
                "required": ["message_id", "emoji", "chat_id"],
            }),
        }
    }

    fn call<'a>(
        &self,
        args: Value,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>> {
        let bot = self.bot.clone();

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
                .ok_or_else(|| ToolError::Failed("missing 'chat_id' argument".to_owned()))?;

            bot.set_message_reaction(chat_id, MessageId(message_id))
                .reaction(vec![ReactionType::Emoji { emoji }])
                .await
                .map_err(|e| ToolError::Failed(e.to_string()))?;

            Ok("реакция поставлена".to_owned())
        })
    }
}
