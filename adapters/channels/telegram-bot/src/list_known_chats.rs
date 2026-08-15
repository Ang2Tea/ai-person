use contracts::{Storage, Tool, ToolError, ToolSpec};
use serde_json::{Value, json};
use std::future::Future;
use std::pin::Pin;

use crate::history::BufferStore;

pub struct ListKnownChats<S> {
    history: BufferStore<S>,
}

impl<S> ListKnownChats<S> {
    pub fn new(history: BufferStore<S>) -> Self {
        Self { history }
    }
}

impl<S> Tool for ListKnownChats<S>
where
    S: Storage + Clone + Send + Sync + 'static,
{
    fn name(&self) -> &str {
        "list_known_chats"
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "list_known_chats".to_owned(),
            description: "Показывает список чатов Telegram, с которыми ты уже когда-либо \
        общался (id, последний известный собеседник и сколько сообщений от него накопилось \
        без ответа). Используй перед `send_message` с явным `chat_id`, чтобы выбрать, куда именно \
        писать — по памяти id чатов не угадать; счётчик непрочитанных подскажет, где давно ждут ответа."
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
        let history = self.history.clone();

        Box::pin(async move {
            let chat_ids = history.chat_ids().await;
            if chat_ids.is_empty() {
                return Ok("известных чатов пока нет".to_owned());
            }

            let mut lines = Vec::new();
            for chat_id in chat_ids {
                let buffer = history.get(chat_id).await;
                let label = buffer
                    .as_ref()
                    .and_then(|b| b.last_sender_name().map(str::to_owned))
                    .unwrap_or_else(|| "неизвестно".to_owned());
                let unread = buffer.as_ref().map(|b| b.unread_count()).unwrap_or(0);
                let kind = if chat_id < 0 {
                    "группа"
                } else {
                    "личный чат"
                };
                lines.push(format!(
                    "{chat_id}: {kind}, последний собеседник — {label}, непрочитанных: {unread}"
                ));
            }

            Ok(lines.join("\n"))
        })
    }
}
