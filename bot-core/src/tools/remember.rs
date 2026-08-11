use contracts::{Memory, Tool, ToolError, ToolSpec};
use serde_json::{Value, json};
use std::future::Future;
use std::pin::Pin;

pub struct Remember<M> {
    memory: M,
}

impl<M> Remember<M> {
    pub fn new(memory: M) -> Self {
        Self { memory }
    }
}

impl<M> Tool for Remember<M>
where
    M: Memory + Clone + Send + Sync + 'static,
{
    fn name(&self) -> &str {
        "remember"
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "remember".to_owned(),
            description: "Сохранить важный факт в долгосрочную память прямо сейчас, \
не дожидаясь фоновой архивации. Используй по прямой просьбе собеседника запомнить что-то, \
или когда сам считаешь факт важным и не хочешь полагаться на фоновую выгрузку — не для \
рутинной информации на каждую реплику."
                .to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "Текст факта, который нужно запомнить",
                    },
                    "chat_id": {
                        "type": "integer",
                        "description": "id чата, к которому относится факт",
                    },
                    "confidence": {
                        "type": "number",
                        "description": "-1 (заведомая ложь) .. 1 (точно подтверждено), 0 по умолчанию",
                    },
                    "visibility": {
                        "type": "string",
                        "enum": ["private", "public"],
                        "description": "private по умолчанию, public — только если факт точно нейтрален",
                    },
                    "about_users": {
                        "type": "array",
                        "items": { "type": "integer" },
                        "description": "Telegram user_id, если факт о конкретных людях в группе",
                    },
                },
                "required": ["text", "chat_id"],
            }),
        }
    }

    fn call<'a>(
        &self,
        args: Value,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>> {
        let memory = self.memory.clone();

        Box::pin(async move {
            let text = args
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| ToolError::Failed("missing 'text' argument".to_owned()))?
                .to_owned();
            let chat_id = args
                .get("chat_id")
                .and_then(Value::as_i64)
                .ok_or_else(|| ToolError::Failed("missing 'chat_id' argument".to_owned()))?;
            let confidence = args
                .get("confidence")
                .and_then(Value::as_f64)
                .unwrap_or(0.0) as f32;
            let visibility_public = matches!(args.get("visibility").and_then(Value::as_str), Some("public"));
            let about_users: Vec<i64> = args
                .get("about_users")
                .and_then(Value::as_array)
                .map(|items| items.iter().filter_map(Value::as_i64).collect())
                .unwrap_or_default();

            let saved = memory
                .remember(chat_id, &text, confidence, visibility_public, &about_users)
                .await;

            Ok(if saved {
                "запомнено".to_owned()
            } else {
                "уже было похожее, не сохранено".to_owned()
            })
        })
    }
}
