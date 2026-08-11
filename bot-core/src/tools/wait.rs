use contracts::{Tool, ToolError, ToolSpec};
use serde_json::{Value, json};
use std::future::Future;
use std::pin::Pin;

pub struct Wait;

impl Tool for Wait {
    fn name(&self) -> &str {
        "wait"
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "wait".to_owned(),
            description: "Промолчать в этом ходу — не отправлять сообщение пользователю прямо сейчас."
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
        Box::pin(async move { Ok("ok, staying silent".to_owned()) })
    }

    fn ends_turn(&self) -> bool {
        true
    }
}
