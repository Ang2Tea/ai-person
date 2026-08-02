use serde_json::{Value, json};
use std::pin::Pin;

use crate::buffer::BufferStore;
use crate::contracts::BufferStorage;
use crate::errors::ToolError;
use crate::tools::{Tool, ToolContext};

pub struct ReadChatHistory<B> {
    buffer: BufferStore<B>,
}

impl<B> ReadChatHistory<B> {
    pub fn new(buffer: BufferStore<B>) -> Self {
        Self { buffer }
    }
}

impl<B> Tool<B> for ReadChatHistory<B>
where
    B: BufferStorage + Clone + Send + Sync + 'static,
{
    fn name(&self) -> &str {
        "read_chat_history"
    }

    fn spec(&self) -> Value {
        json!({
            "type": "function",
            "function": {
                "name": "read_chat_history",
                "description": "Показывает недавнюю переписку из другого известного чата (не \
текущего) — по id, который можно узнать через `list_known_chats`. Используй, когда нужно \
продолжить тему из другого разговора или явно попросили посмотреть переписку с кем-то — не \
читай чужие чаты просто из любопытства.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "chat_id": {
                            "type": "integer",
                            "description": "id чата, чью историю нужно посмотреть",
                        },
                    },
                    "required": ["chat_id"],
                },
            },
        })
    }

    fn call<'a>(
        &self,
        args: Value,
        _ctx: &ToolContext<B>,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>> {
        let buffer = self.buffer.clone();

        Box::pin(async move {
            let chat_id = args
                .get("chat_id")
                .and_then(Value::as_i64)
                .ok_or_else(|| ToolError::Failed("missing 'chat_id' argument".to_owned()))?;

            match buffer.get(chat_id).await {
                Some(chat_buffer) => {
                    let transcript = chat_buffer.to_transcript();
                    if transcript.is_empty() {
                        Ok("в этом чате пока пусто".to_owned())
                    } else {
                        Ok(transcript)
                    }
                }
                None => Ok("такой чат не найден".to_owned()),
            }
        })
    }
}
