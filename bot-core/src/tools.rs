mod get_current_datetime;
mod remember;
mod wait;

pub use get_current_datetime::GetCurrentDatetime;
pub use remember::Remember;
pub use wait::Wait;

use contracts::{Memory, Tool, ToolCall, ToolSpec};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Channel-агностичные инструменты — не зависят ни от какого конкретного
/// канала связи, регистрируются вместе с инструментами, которые поставляет
/// сам канал (например `channel_telegram_bot::tools`).
pub fn tools<M>(memory: M) -> Vec<Arc<dyn Tool>>
where
    M: Memory + Clone + Send + Sync + 'static,
{
    vec![
        Arc::new(GetCurrentDatetime),
        Arc::new(Wait),
        Arc::new(Remember::new(memory)),
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

    pub fn ends_turn(&self, name: &str) -> bool {
        self.tools.get(name).is_some_and(|t| t.ends_turn())
    }

    #[tracing::instrument(level = "debug", skip(self, call), fields(tool = %call.name))]
    pub async fn dispatch(&self, call: &ToolCall) -> String {
        let Some(tool) = self.tools.get(&call.name) else {
            tracing::error!("unknown tool requested by model");
            return format!("error: unknown tool '{}'", call.name);
        };
        let args: Value = match serde_json::from_str(&call.arguments) {
            Ok(v) => v,
            Err(err) => {
                tracing::error!(%err, arguments = %call.arguments, "bad tool arguments json");
                return format!("error: bad arguments json: {err}");
            }
        };
        match tool.call(args).await {
            Ok(s) => s,
            Err(err) => {
                tracing::error!(%err, arguments = %call.arguments, "tool call failed");
                format!("error: {err}")
            }
        }
    }
}
