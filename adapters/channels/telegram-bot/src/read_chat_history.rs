use contracts::{Storage, Tool, ToolError, ToolSpec};
use serde_json::{Value, json};
use std::future::Future;
use std::pin::Pin;

use crate::history::BufferStore;

pub struct ReadChatHistory<S> {
    history: BufferStore<S>,
}

impl<S> ReadChatHistory<S> {
    pub fn new(history: BufferStore<S>) -> Self {
        Self { history }
    }
}

impl<S> Tool for ReadChatHistory<S>
where
    S: Storage + Clone + Send + Sync + 'static,
{
    fn name(&self) -> &str {
        "read_chat_history"
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "read_chat_history".to_owned(),
            description: "Показывает недавнюю переписку из известного чата — по id, который \
можно узнать через `list_known_chats`. Используй, когда нужно продолжить тему из другого \
разговора или явно попросили посмотреть переписку с кем-то — не читай чужие чаты просто из \
любопытства."
                .to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "chat_id": {
                        "type": "integer",
                        "description": "id чата, чью историю нужно посмотреть",
                    },
                },
                "required": ["chat_id"],
            }),
        }
    }

    fn call<'a>(
        &self,
        args: Value,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>> {
        let history = self.history.clone();

        Box::pin(async move {
            let chat_id = args
                .get("chat_id")
                .and_then(Value::as_i64)
                .ok_or_else(|| ToolError::Failed("missing 'chat_id' argument".to_owned()))?;

            match history.get(chat_id).await {
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
