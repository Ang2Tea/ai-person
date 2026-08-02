mod get_current_datetime;
mod send_message;
mod wait;

pub use get_current_datetime::GetCurrentDatetime;
pub use send_message::SendMessage;
pub use wait::Wait;

use serde_json::Value;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use crate::contracts::ToolCall;
use crate::errors::ToolError;

pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn spec(&self) -> Value;
    fn call<'a>(
        &self,
        args: Value,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>>;
}

#[derive(Default)]
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn specs(&self) -> Vec<Value> {
        self.tools.values().map(|t| t.spec()).collect()
    }

    pub async fn dispatch(&self, call: &ToolCall) -> String {
        let Some(tool) = self.tools.get(&call.function.name) else {
            return format!("error: unknown tool '{}'", call.function.name);
        };
        let args: Value = match serde_json::from_str(&call.function.arguments) {
            Ok(v) => v,
            Err(e) => return format!("error: bad arguments json: {e}"),
        };
        match tool.call(args).await {
            Ok(s) => s,
            Err(e) => format!("error: {e}"),
        }
    }
}
