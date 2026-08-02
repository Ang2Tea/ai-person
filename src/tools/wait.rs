use serde_json::{Value, json};
use std::pin::Pin;

use crate::errors::ToolError;
use crate::tools::Tool;

pub struct Wait;

impl Tool for Wait {
    fn name(&self) -> &str {
        "wait"
    }

    fn spec(&self) -> Value {
        json!({
            "type": "function",
            "function": {
                "name": "wait",
                "description": "Промолчать в этом ходу — не отправлять сообщение пользователю прямо сейчас.",
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
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>> {
        Box::pin(async move { Ok("ok, staying silent".to_owned()) })
    }
}
