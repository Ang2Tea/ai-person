use contracts::{Storage, Tool, ToolError, ToolSpec};
use serde_json::{Value, json};
use std::future::Future;
use std::pin::Pin;

use crate::buffer::BufferStore;

pub struct ListKnownChats<B> {
    buffer: BufferStore<B>,
}

impl<B> ListKnownChats<B> {
    pub fn new(buffer: BufferStore<B>) -> Self {
        Self { buffer }
    }
}

impl<B> Tool for ListKnownChats<B>
where
    B: Storage + Clone + Send + Sync + 'static,
{
    fn name(&self) -> &str {
        "list_known_chats"
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "list_known_chats".to_owned(),
            description: "Показывает список чатов Telegram, с которыми ты уже когда-либо \
        общался (id и последний известный собеседник в этом чате). Используй перед `send_message` с \
        явным `chat_id`, чтобы выбрать, куда именно писать — по памяти id чатов не угадать."
                .to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {},
                "required": [],
            }),
        }
    }

    fn call<'a>(
        &self,
        _args: Value,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>> {
        let buffer = self.buffer.clone();

        Box::pin(async move {
            let chat_ids = buffer.chat_ids().await;
            if chat_ids.is_empty() {
                return Ok("известных чатов пока нет".to_owned());
            }

            let mut lines = Vec::new();
            for chat_id in chat_ids {
                let label = buffer
                    .get(chat_id)
                    .await
                    .and_then(|b| b.last_sender_name().map(str::to_owned))
                    .unwrap_or_else(|| "неизвестно".to_owned());
                let kind = if chat_id < 0 {
                    "группа"
                } else {
                    "личный чат"
                };
                lines.push(format!("{chat_id}: {kind}, последний собеседник — {label}"));
            }

            Ok(lines.join("\n"))
        })
    }
}
