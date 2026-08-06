mod get_current_datetime;
mod list_known_chats;
mod read_chat_history;
mod remember;
mod wait;

pub use get_current_datetime::GetCurrentDatetime;
pub use list_known_chats::ListKnownChats;
pub use read_chat_history::ReadChatHistory;
pub use remember::Remember;
pub use wait::Wait;

use contracts::{Llm, Storage, Tool, ToolCall, ToolSpec};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

use crate::buffer::BufferStore;
use crate::memory::MemoryStore;
use crate::settings::MemorySettings;

/// Channel-агностике инструменты — не зависят ни от какого конкретного
/// канала связи, регистрируются вместе с инструментами, которые поставляет
/// сам канал (например `channel_telegram_bot::tools`).
pub fn tools<L, B>(
    buffer: BufferStore<B>,
    llm: L,
    memory: MemoryStore<B>,
    memory_settings: MemorySettings,
    embedding_model: Arc<str>,
) -> Vec<Arc<dyn Tool>>
where
    L: Llm + Clone + Send + Sync + 'static,
    B: Storage + Clone + Send + Sync + 'static,
{
    vec![
        Arc::new(GetCurrentDatetime),
        Arc::new(Wait),
        Arc::new(ListKnownChats::new(buffer.clone())),
        Arc::new(ReadChatHistory::new(buffer)),
        Arc::new(Remember::new(llm, memory, memory_settings, embedding_model)),
    ]
}

#[derive(Default)]
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ключ — `spec().name`, а не `tool.name()`: модель зовёт инструмент по
    /// имени из `spec()` (это же имя разослано ей в списке доступных tools),
    /// `tool.name()` — внутренний идентификатор, который может отличаться
    /// (например, с префиксом канала — `telegram_send_message`), чтобы не
    /// конфликтовать с одноимёнными инструментами других каналов.
    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.spec().name.clone(), tool);
    }

    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools.values().map(|t| t.spec()).collect()
    }

    pub async fn dispatch(&self, call: &ToolCall) -> String {
        let Some(tool) = self.tools.get(&call.name) else {
            tracing::error!(tool = %call.name, "unknown tool requested by model");
            return format!("error: unknown tool '{}'", call.name);
        };
        let args: Value = match serde_json::from_str(&call.arguments) {
            Ok(v) => v,
            Err(err) => {
                tracing::error!(tool = %call.name, %err, arguments = %call.arguments, "bad tool arguments json");
                return format!("error: bad arguments json: {err}");
            }
        };
        match tool.call(args).await {
            Ok(s) => s,
            Err(err) => {
                tracing::error!(tool = %call.name, %err, "tool call failed");
                format!("error: {err}")
            }
        }
    }
}
