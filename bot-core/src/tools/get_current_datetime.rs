use chrono::Utc;
use contracts::{Tool, ToolError, ToolSpec};
use serde_json::{Value, json};
use std::future::Future;
use std::pin::Pin;

pub struct GetCurrentDatetime;

impl Tool for GetCurrentDatetime {
    fn name(&self) -> &str {
        "get_current_datetime"
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "get_current_datetime".to_owned(),
            description: "Возвращает текущую дату и время в UTC.".to_owned(),
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
        Box::pin(async move { Ok(Utc::now().to_rfc3339()) })
    }
}
