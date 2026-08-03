mod access;
mod context;
mod get_current_datetime;
mod list_known_chats;
mod read_chat_history;
mod remember;
mod search_memory;
mod send_message;
mod send_reaction;
mod wait;

pub use access::is_chat_access_allowed;
pub use context::ToolContext;
pub use get_current_datetime::GetCurrentDatetime;
pub use list_known_chats::ListKnownChats;
pub use read_chat_history::ReadChatHistory;
pub use remember::Remember;
pub use search_memory::SearchMemory;
pub use send_message::SendMessage;
pub use send_reaction::SendReaction;
pub use wait::Wait;

use serde_json::Value;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use crate::contracts::ToolCall;
use crate::errors::ToolError;

pub trait Tool<B>: Send + Sync {
    fn name(&self) -> &str;
    fn spec(&self) -> Value;
    fn call<'a>(
        &self,
        args: Value,
        ctx: &ToolContext<B>,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>>;
}

pub struct ToolRegistry<B> {
    tools: HashMap<String, Arc<dyn Tool<B>>>,
}

impl<B> Default for ToolRegistry<B> {
    fn default() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }
}

impl<B> ToolRegistry<B> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, tool: Arc<dyn Tool<B>>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn specs(&self) -> Vec<Value> {
        self.tools.values().map(|t| t.spec()).collect()
    }

    pub async fn dispatch(&self, call: &ToolCall, ctx: &ToolContext<B>) -> String {
        let Some(tool) = self.tools.get(&call.function.name) else {
            tracing::error!(tool = %call.function.name, "unknown tool requested by model");
            return format!("error: unknown tool '{}'", call.function.name);
        };
        let args: Value = match serde_json::from_str(&call.function.arguments) {
            Ok(v) => v,
            Err(err) => {
                tracing::error!(tool = %call.function.name, %err, arguments = %call.function.arguments, "bad tool arguments json");
                return format!("error: bad arguments json: {err}");
            }
        };
        match tool.call(args, ctx).await {
            Ok(s) => s,
            Err(err) => {
                tracing::error!(tool = %call.function.name, %err, "tool call failed");
                format!("error: {err}")
            }
        }
    }
}
