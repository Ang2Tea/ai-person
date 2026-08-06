use chrono::Utc;
use serde_json::{Value, json};
use std::pin::Pin;

use crate::errors::ToolError;
use crate::tools::{Tool, ToolContext};

pub struct GetCurrentDatetime;

impl<B: Send + Sync + 'static> Tool<B> for GetCurrentDatetime {
    fn name(&self) -> &str {
        "get_current_datetime"
    }

    fn spec(&self) -> Value {
        json!({
            "type": "function",
            "function": {
                "name": "get_current_datetime",
                "description": "Возвращает текущую дату и время в UTC.",
                "parameters": {
                    "type": "object",
                    "properties": {},
                    "required": [],
                },
            },
        })
    }

    fn call<'a>(
        &self,
        _args: Value,
        _ctx: &ToolContext<B>,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>> {
        Box::pin(async move { Ok(Utc::now().to_rfc3339()) })
    }
}
